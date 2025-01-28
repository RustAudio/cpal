use quote::ToTokens;
use quote::__private::TokenStream;
use std::str::FromStr;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::{Bracket, Paren, Pound};
use syn::{AttrStyle, Attribute, Ident, MacroDelimiter, Meta, MetaList, PathArguments, PathSegment};

fn add_attribute(attrs: &mut Vec<Attribute>, ident: &str, value: &str) {
    attrs.push(
        Attribute {
            pound_token: Pound::default(),
            style: AttrStyle::Outer,
            bracket_token: Bracket::default(),
            meta: Meta::List(MetaList {
                path: syn::Path {
                    leading_colon: None,
                    segments: {
                        let mut segments = Punctuated::default();
                        let ident = ident;
                        segments.push(PathSegment {
                            ident: Ident::new(ident, ident.span()),
                            arguments: PathArguments::None,
                        });
                        segments
                    },
                },
                delimiter: MacroDelimiter::Paren(Paren::default()),
                tokens: TokenStream::from_str(value).unwrap(),
            }),
        }
    );
}

pub trait AttributeExt {
    fn push_one(&mut self, ident: &str, value: &str);
    fn to_token_stream(&self) -> TokenStream;
    fn contains(&self, ident: &String) -> bool;
}

impl AttributeExt for Vec<Attribute> {
    fn push_one(&mut self, ident: &str, value: &str) {
        add_attribute(self, ident, value);
    }

    fn to_token_stream(&self) -> TokenStream {
        self.iter()
            .map(|attr| attr.to_token_stream())
            .collect()
    }

    fn contains(&self, ident: &String) -> bool {
        self.iter().any(move |attribute| {
            match &attribute.meta {
                Meta::Path(value) => {
                    value.segments.iter().any(|segment| segment.ident == ident)
                }
                Meta::List(value) => {
                    value.path.segments.iter().any(|segment| segment.ident == ident)
                }
                Meta::NameValue(value) => {
                    value.path.segments.iter().any(|segment| segment.ident == ident)
                }
            }
        })
    }
}