//! No-op instrumentation attribute used when GPUI tracing is disabled.

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn instrument(_attribute: TokenStream, item: TokenStream) -> TokenStream {
    item
}
