use proc_macro::TokenStream;
use quote::quote;

#[proc_macro_attribute]
pub fn cpu_local(_: TokenStream, input: TokenStream) -> TokenStream {
    let input = proc_macro2::TokenStream::from(input);

    quote! {
        #[thread_local]
        #[link_section = ".cls"]
        #input
    }
    .into()
}
