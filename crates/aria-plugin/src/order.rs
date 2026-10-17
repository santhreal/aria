//! What order plugins mount in.
//!
//! [`PluginContext::mount`](crate::PluginContext::mount) refuses a plugin whose
//! declared dependency is not mounted yet, so the order a host mounts in used to
//! be load-bearing: a hand-written list in the wrong order failed a mount for a
//! reason that read like a missing plugin. The order is derived from the declared
//! dependencies here instead, so a host lists its plugins in any order.
//!
//! The result is deterministic: among plugins that do not depend on each other,
//! the declared order is kept. A run that mounted in one order must not mount in
//! another on the next start, because mount order decides event-handler order.
//!
//! A dependency on a plugin that is not in the set is left alone rather than
//! treated as an ordering failure — `mount` is the one owner of "that plugin is
//! not here", and it names the dependency.

use crate::error::PluginError;
use crate::plugin::PluginManifest;

/// The order to mount `manifests` in, or the cycle that makes an order
/// impossible.
///
/// Every id in the input appears exactly once in the output.
pub fn load_order(manifests: &[PluginManifest]) -> Result<Vec<&str>, PluginError> {
    let ids: Vec<&str> = manifests.iter().map(|m| m.id.as_ref()).collect();
    let mut mounted: Vec<&str> = Vec::with_capacity(ids.len());
    let mut pending: Vec<&PluginManifest> = manifests.iter().collect();

    while !pending.is_empty() {
        // Take the first plugin whose in-set dependencies are all mounted. Taking
        // the *first* is what keeps the declared order among independent plugins.
        let next = pending.iter().position(|manifest| {
            manifest
                .requires
                .iter()
                .all(|dep| !ids.contains(&dep) || mounted.contains(&dep))
        });
        match next {
            Some(index) => {
                let manifest = pending.remove(index);
                mounted.push(manifest.id.as_ref());
            }
            // Nothing is ready and plugins remain: every one of them is waiting on
            // another that is also waiting. That is the cycle.
            None => {
                let names: Vec<&str> = pending.iter().map(|m| m.id.as_ref()).collect();
                return Err(PluginError::DependencyCycle(names.join(" -> ")));
            }
        }
    }
    Ok(mounted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(id: &'static str, requires: &'static [&'static str]) -> PluginManifest {
        PluginManifest::new(id, id, "0.1.0", requires, &[])
    }

    /// The point of the function: a host lists plugins in any order and the one
    /// that is depended on still mounts first.
    #[test]
    fn a_dependency_mounts_before_the_plugin_that_declares_it() {
        let declared = [
            manifest("app", &["engine"]),
            manifest("engine", &["storage"]),
            manifest("storage", &[]),
        ];
        let order = load_order(&declared).expect("orderable");
        assert_eq!(order, vec!["storage", "engine", "app"]);
    }

    /// Mount order decides event-handler order, so two independent plugins must
    /// keep the order the host declared rather than any order that satisfies the
    /// graph.
    #[test]
    fn independent_plugins_keep_their_declared_order() {
        let declared = [
            manifest("b", &[]),
            manifest("a", &[]),
            manifest("c", &["a"]),
        ];
        let order = load_order(&declared).expect("orderable");
        assert_eq!(order, vec!["b", "a", "c"]);
        // Same input, same answer, every time.
        assert_eq!(load_order(&declared).expect("orderable"), order);
    }

    /// A cycle has no valid order at all, so it is refused whole and the message
    /// names every plugin in it — naming one would send a reader looking for a
    /// missing plugin that is not missing.
    #[test]
    fn a_cycle_is_refused_and_names_its_members() {
        let err = load_order(&[
            manifest("standalone", &[]),
            manifest("left", &["right"]),
            manifest("right", &["left"]),
        ])
        .expect_err("a cycle has no order");
        let text = err.to_string();
        assert!(text.contains("left"), "{text}");
        assert!(text.contains("right"), "{text}");
        assert!(
            !text.contains("standalone"),
            "a plugin outside the cycle is not in it: {text}"
        );
    }

    /// A plugin that requires itself can never be ready, which is the same defect
    /// as a two-plugin loop and must not hang or drop the plugin silently.
    #[test]
    fn a_self_dependency_is_a_cycle() {
        let err = load_order(&[manifest("loop", &["loop"])]).expect_err("self-cycle");
        assert!(err.to_string().contains("loop"));
    }

    /// A dependency outside the set belongs to `mount`, which names it. Ordering
    /// must not swallow the plugin or refuse the whole batch over it.
    #[test]
    fn an_unknown_dependency_still_orders_the_rest() {
        let declared = [
            manifest("needs_absent", &["not.here"]),
            manifest("plain", &[]),
        ];
        let order = load_order(&declared).expect("an absent dependency is not an ordering failure");
        assert_eq!(order, vec!["needs_absent", "plain"]);
    }

    /// Every id in, every id out, once: a host mounts exactly what it listed.
    #[test]
    fn every_plugin_appears_exactly_once() {
        let declared = [
            manifest("d", &["c"]),
            manifest("c", &["b"]),
            manifest("b", &["a"]),
            manifest("a", &[]),
            manifest("e", &["a", "c"]),
        ];
        let order = load_order(&declared).expect("orderable");
        assert_eq!(order.len(), declared.len());
        for m in &declared {
            assert_eq!(
                order.iter().filter(|id| **id == m.id).count(),
                1,
                "{} appears more than once",
                m.id
            );
        }
        for m in &declared {
            let at = order.iter().position(|id| *id == m.id).expect("present");
            for dep in m.requires.iter() {
                let dep_at = order.iter().position(|id| *id == dep).expect("dep present");
                assert!(dep_at < at, "{} mounted before its dependency {dep}", m.id);
            }
        }
    }
}
