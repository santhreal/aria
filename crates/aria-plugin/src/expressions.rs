//! `aria.expressions`: the driver language Aria ships, a JavaScript subset with
//! the After Effects expression library.
//!
//! Source is lexed, parsed, and bound once per text: binding turns every
//! property reference into a declared input, so the renderer orders drivers
//! and refuses cycles before a frame is drawn. Evaluation walks the bound tree
//! against the renderer's [`DriverScope`], checks the scope's clock every few
//! hundred steps, and stops after [`MAX_STEPS`](eval::MAX_STEPS) steps under
//! any clock.
//!
//! The language reference is `docs/plugins/expressions.md`.

mod bind;
mod eval;
mod lex;
mod parse;

use std::sync::Arc;

use aria_models::ParamValue;
use aria_render::{DriverInput, DriverLanguage, DriverProgram, DriverScope, InputClip};

use crate::drivers::DriverLanguageSpec;
use crate::error::PluginError;
use crate::{Plugin, PluginContext, PluginManifest};

/// The id driver code written in `aria.expressions` names.
pub const EXPRESSIONS_LANGUAGE: &str = "aria.expressions";

/// The `aria.expressions` driver language.
#[derive(Debug, Clone, Copy, Default)]
pub struct Expressions;

impl DriverLanguage for Expressions {
    fn compile(&self, source: &str) -> Result<Arc<dyn DriverProgram>, String> {
        let body = parse::parse(lex::lex(source)?)?;
        if body.is_empty() {
            return Err("the driver is empty: write an expression, such as value".into());
        }
        let bound = bind::bind(body)?;
        Ok(Arc::new(Program {
            body: bound.body,
            inputs: bound.inputs,
            units: bound.units,
            clips: bound.clips,
        }))
    }

    fn reference(&self, input: &DriverInput) -> Option<String> {
        Some(bind::reference(input))
    }

    fn template(&self) -> String {
        "value".into()
    }
}

/// Compiled `aria.expressions` source.
struct Program {
    body: Vec<parse::Stmt>,
    inputs: Vec<DriverInput>,
    /// The units of each input, by index.
    units: Vec<bind::Units>,
    /// The clips whose name and extent the program reads.
    clips: Vec<InputClip>,
}

impl DriverProgram for Program {
    fn inputs(&self) -> &[DriverInput] {
        &self.inputs
    }

    fn evaluate(&self, scope: &mut dyn DriverScope) -> Result<ParamValue, String> {
        eval::evaluate(self, scope)
    }
}

/// The built-in plugin that registers [`Expressions`] under
/// [`EXPRESSIONS_LANGUAGE`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ExpressionsPlugin;

impl Plugin for ExpressionsPlugin {
    fn manifest(&self) -> &PluginManifest {
        static MANIFEST: PluginManifest = PluginManifest::new(
            EXPRESSIONS_LANGUAGE,
            "Expressions",
            env!("CARGO_PKG_VERSION"),
            &[],
            &[],
        );
        &MANIFEST
    }

    fn mount(&self, ctx: &PluginContext) -> Result<(), PluginError> {
        ctx.register_driver_language(
            DriverLanguageSpec {
                id: EXPRESSIONS_LANGUAGE.into(),
                name: "Expressions".into(),
            },
            Expressions,
        )?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "expressions/tests.rs"]
mod tests;
