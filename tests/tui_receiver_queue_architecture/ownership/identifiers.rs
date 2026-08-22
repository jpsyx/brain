use syn::Ident;
use syn::ext::IdentExt;

pub(super) fn canonical_ident(ident: &Ident) -> String {
    ident.unraw().to_string()
}

pub(super) fn ident_is(ident: &Ident, expected: &str) -> bool {
    ident.unraw() == expected
}
