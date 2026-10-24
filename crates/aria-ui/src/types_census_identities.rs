use super::*;

/// `text` with every whole-line comment blanked.
///
/// Two reasons: a rule about `format!` is described in prose a file away and
/// must not match itself, and a comment's parentheses would otherwise unbalance
/// the span the next rule matches.
fn without_comments(text: &str) -> String {
    text.lines()
        .map(|line| {
            if line.trim_start().starts_with("//") {
                ""
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every element-identity construction in `text`, as `(line number, source)`.
///
/// A construction is `.id(`, `.with_animation(` or `ElementId::` followed by its
/// balanced argument list, so an id split over four lines is read as one thing
/// and a nested call inside it does not end the span early. A type annotation
/// such as `id: gpui::ElementId,` is not a construction, and is skipped because
/// the text between the name and the next `(` carries a separator no call would
/// have.
///
/// An animation is one of these because GPUI keys animation state by element id
/// exactly as it keys interaction state, so a one-shot transition named with
/// `format!` allocates on every frame it draws, which is every frame it runs.
fn identity_spans(text: &str) -> Vec<(usize, String)> {
    const NEEDLES: [&str; 3] = [".id(", "ElementId::", ".with_animation("];
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    for needle in NEEDLES {
        let mut from = 0;
        while let Some(found) = text[from..].find(needle) {
            let start = from + found;
            from = start + needle.len();
            let after = start + needle.len();
            let Some(open) = text[start..].find('(').map(|at| start + at).filter(|open| {
                *open <= after || !text[after..*open].contains([',', ';', ')', '\n'])
            }) else {
                continue;
            };
            let mut depth = 0usize;
            let mut end = open + 1;
            for (offset, byte) in bytes[open..].iter().enumerate() {
                match byte {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + offset + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            spans.push((text[..start].lines().count(), text[start..end].to_string()));
        }
    }
    spans
}

/// **An element identity is never built out of text.**
///
/// `ElementId` converts from `String` and so does `SharedString`, so a control
/// could name itself with `format!` — and fourteen did, once per multicam angle,
/// mixer strip, meter, fader, live job, curve parameter and easing preset, on
/// every frame each drew. The loaders then hashed the same text straight back
/// into a motion key, so the string was allocated to be thrown away.
///
/// The shared controls take a `motion::ControlId` now, which converts from a
/// static name or a `MotionKey` and from nothing else, so those call sites
/// cannot return. A raw `.id(…)` on a `div` still accepts an owned string, which
/// is what this counts.
#[test]
fn no_element_identity_is_built_from_a_formatted_string() {
    let mut built = Vec::new();
    for (name, text) in draw_sources() {
        for (line, span) in identity_spans(&without_comments(&text)) {
            if span.contains("format!") {
                built.push(format!("{name}:{line}: {}", span.replace('\n', " ")));
            }
        }
    }
    assert!(
        built.is_empty(),
        "element identities formatted on every frame they draw: {}; hash the \
         parts with `MotionKey::new` and pass the key, or name the control with a \
         literal:\n  {}",
        built.len(),
        built.join("\n  ")
    );
}

/// The span of `text` from `at` up to the first `;` outside every bracket.
///
/// A statement rather than a line, so an initialiser that runs over four lines
/// is read as one expression and a `;` inside a closure does not end it early.
fn statement(text: &str, at: usize) -> &str {
    let mut depth = 0i32;
    for (offset, byte) in text.as_bytes()[at..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth -= 1,
            b';' if depth == 0 => return &text[at..at + offset],
            _ => {}
        }
    }
    &text[at..]
}

/// The span from the bracket at `open` to the one that closes it, inclusive.
fn bracketed(text: &str, open: usize) -> &str {
    let mut depth = 0i32;
    for (offset, byte) in text.as_bytes()[open..].iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return &text[open..=open + offset];
                }
            }
            _ => {}
        }
    }
    &text[open..]
}

/// Whether `name` says the value it holds is a control's identity.
fn names_an_identity(name: &str) -> bool {
    matches!(name, "id" | "key") || name.ends_with("_id") || name.ends_with("_key")
}

/// Every function in `text`, as `(line number, name, parameter list, body)`.
fn functions(text: &str) -> Vec<(usize, &str, &str, &str)> {
    let mut found = Vec::new();
    for (at, _) in text.match_indices("fn ") {
        let after = at + 3;
        let name_len = text[after..]
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(0);
        let Some(open) = text[after..].find('(').map(|offset| after + offset) else {
            continue;
        };
        let parameters = bracketed(text, open);
        let rest = open + parameters.len();
        // A trait method with no body, or a function pointer type: nothing draws.
        let Some(brace) = text[rest..].find('{').map(|offset| rest + offset) else {
            continue;
        };
        if text[rest..brace].contains(';') {
            continue;
        }
        found.push((
            text[..at].lines().count(),
            &text[after..after + name_len],
            parameters,
            bracketed(text, brace),
        ));
    }
    found
}

/// **An identity is not laundered through a local either.**
///
/// The rule above matches the construction, so a `format!` bound to a name and
/// handed over afterwards would slip past it — which is the shape both the
/// colour wheels and the timeline's grips had: `let side_id = if left {
/// format!("trim-l-{id}") } else { … }`, four of them per clip per frame, on the
/// hottest surface in the app. The initialiser is read to its `;`, so the
/// `format!` does not have to sit on the line the name is on.
#[test]
fn no_draw_binds_an_identity_to_a_formatted_string() {
    let mut bound = Vec::new();
    for (name, text) in draw_sources() {
        for (at, _) in text.match_indices("let ") {
            let bound_at = statement(&text, at);
            let Some((head, value)) = bound_at.split_once('=') else {
                continue;
            };
            if !value.contains("format!") {
                continue;
            }
            let declared = head.split(':').next().unwrap_or(head).trim();
            let local = declared.rsplit(' ').next().unwrap_or(declared);
            if names_an_identity(local) {
                bound.push(format!(
                    "{name}:{}: {}",
                    text[..at].lines().count(),
                    bound_at.replace('\n', " ")
                ));
            }
        }
    }
    assert!(
        bound.is_empty(),
        "identities in the draw layer built from text and then passed on: {}; \
         build a `MotionKey` from the parts instead:\n  {}",
        bound.len(),
        bound.join("\n  ")
    );
}

/// **A helper that draws a control takes its identity, not text.**
///
/// The third way round the rule, and the one that survived longest: a panel's
/// own `chip` or `item` or `handle_dot` took a `String` and assigned it to an
/// element id itself, so the caller could format one and neither rule above was
/// looking at that argument. The mixer did, once per output chip and once per
/// send chip, per strip, per frame.
///
/// A function is matched when it assigns an element id and names a parameter as
/// an identity, so a new helper is red until its identity is a
/// `motion::ControlId` — which is the only identity type a caller cannot build
/// out of text. Parameters GPUI hands an element itself are prefixed with `_`
/// and are not the control's own identity.
#[test]
fn no_draw_helper_names_its_control_with_text() {
    const TEXT: [&str; 3] = ["String", "SharedString", "ElementId"];
    let mut helpers = Vec::new();
    for (name, text) in draw_sources() {
        for (line, function, parameters, body) in functions(&text) {
            // `.id()` with nothing between the parentheses is a getter, such as
            // a label colour's `color.id()`, and assigns no element id.
            let assigns_id = body
                .match_indices(".id(")
                .any(|(at, needle)| !body[at + needle.len()..].starts_with(')'));
            if !assigns_id && !body.contains(".with_animation(") {
                continue;
            }
            for parameter in parameters.split(',') {
                let Some((held, kind)) = parameter.split_once(':') else {
                    continue;
                };
                let held = held.trim_start_matches(['(', ' ', '\n']).trim();
                if held.starts_with('_') || !names_an_identity(held) {
                    continue;
                }
                // `GlobalElementId` and `InspectorElementId` are GPUI's own,
                // handed to an element's layout rather than chosen by a caller.
                if kind.contains("GlobalElementId") || kind.contains("InspectorElementId") {
                    continue;
                }
                let borrowed_text = kind.trim_start().starts_with('&') && kind.contains("str");
                if borrowed_text || TEXT.iter().any(|shape| kind.contains(shape)) {
                    helpers.push(format!("{name}:{line}: fn {function}({held}:{kind})"));
                }
            }
        }
    }
    assert!(
        helpers.is_empty(),
        "draw helpers that assign an element id from an identity they took as text, so a \
         caller can format one: {}; take a `motion::ControlId`:\n  {}",
        helpers.len(),
        helpers.join("\n  ")
    );
}

/// **A function that hands back an identity does not build one out of text.**
///
/// The two rules above catch a control naming itself where it draws. This
/// catches the same thing one call away: a helper named `guide_id`, `frame_id`
/// or `stop_id` whose answer is a `format!`. The snap guide did exactly that,
/// once per magnet, on every mouse move of every drag, to name a line that was
/// on screen for a fifth of a second.
///
/// A name is the test rather than the return type, because the return type is
/// what the fix changes: these return a `MotionKey` or a `ControlId`, and
/// neither is reachable from a string.
#[test]
fn no_draw_returns_an_identity_built_from_text() {
    let mut builders = Vec::new();
    for (name, text) in draw_sources() {
        for (line, function, _, body) in functions(&without_comments(&text)) {
            if names_an_identity(function) && body.contains("format!") {
                builders.push(format!("{name}:{line}: fn {function}"));
            }
        }
    }
    assert!(
        builders.is_empty(),
        "functions named for an identity that build one with `format!`, so every caller \
         allocates per frame: {}; return a `MotionKey` or a `motion::ControlId`:\n  {}",
        builders.len(),
        builders.join("\n  ")
    );
}
