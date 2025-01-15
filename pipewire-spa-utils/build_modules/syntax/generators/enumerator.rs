use build_modules::syntax::utils::AttributeExt;
use indexmap::IndexMap;
use quote::ToTokens;
use quote::__private::TokenStream;
use syn::__private::quote::quote;
use syn::__private::TokenStream2;
use syn::punctuated::Punctuated;
use syn::token::{Brace, Enum, Eq, Pub};
use syn::{Attribute, Expr, Fields, Generics, Ident, ItemEnum, Variant, Visibility};
use debug;

#[derive(Debug)]
pub struct EnumInfo {
    pub ident: Ident,
    pub attributes: Vec<Attribute>,
    pub spa_type: Ident,
    pub representation_type: String,
    pub variants: IndexMap<String, EnumVariantInfo>
}

#[derive(Debug)]
pub struct EnumVariantInfo {
    pub attributes: Vec<Attribute>,
    pub fields: Fields,
    pub ident: Ident,
    pub discriminant: Expr
}

impl From<&EnumVariantInfo> for Variant {
    fn from(value: &EnumVariantInfo) -> Self {
        Variant {
            attrs: value.attributes.clone(),
            ident: value.ident.clone(),
            fields: value.fields.clone(),
            discriminant: Some((Eq::default(), value.discriminant.clone())),
        }
    }
}

impl EnumInfo {
    pub fn generate(&self) -> String {
        let mut variants = Punctuated::new();
        self.variants.iter()
            .for_each(|(_, variant)| {
                variants.push(variant.into())
            });
        let mut attributes = self.attributes.clone();
        attributes.push_one("repr", self.representation_type.as_str());
        attributes.push_one("derive", "Debug");
        attributes.push_one("derive", "Clone");
        attributes.push_one("derive", "Copy");
        attributes.push_one("derive", "Ord");
        attributes.push_one("derive", "PartialOrd");
        attributes.push_one("derive", "Eq");
        attributes.push_one("derive", "PartialEq");
        let item = ItemEnum {
            attrs: attributes.clone(),
            vis: Visibility::Public(Pub::default()),
            enum_token: Enum::default(),
            ident: self.ident.clone(),
            generics: Generics::default(),
            brace_token: Brace::default(),
            variants,
        };
        let import_quote = quote! {
            use libspa::sys::*;
        };
        let attributes_quote = self.attributes.to_token_stream();
        let item_quote = quote!(#item);
        let item_ident_quote = item.ident.to_token_stream();
        let representation_type_quote = self.representation_type.parse::<TokenStream2>().unwrap();
        let spa_type_quote = self.spa_type.to_token_stream();
        let from_representation_to_variant_quote = self.variants.iter()
            .map(|(_, variant)| {
                let ident = variant.ident.to_token_stream();
                let discriminant = variant.discriminant.to_token_stream();
                let attributes = variant.attributes.to_token_stream();
                quote! {
                    #attributes
                    #discriminant => Self::#ident,
                }
            })
            .collect::<TokenStream>();
        let from_representation_type_quote = quote! {
            #attributes_quote
            impl From<#representation_type_quote> for #item_ident_quote {
                fn from(value: #representation_type_quote) -> Self {
                    let value: #spa_type_quote = value;
                    match value {
                        #from_representation_to_variant_quote
                        _ => panic!("Unknown variant")
                    }
                }
            }
        };
        let to_representation_type_quote = quote! {
            #attributes_quote
            impl From<&#item_ident_quote> for #representation_type_quote {
                fn from(value: &#item_ident_quote) -> Self {
                    let value: #spa_type_quote = value.into();
                    value
                }
            }
        
            #attributes_quote
            impl From<#item_ident_quote> for #representation_type_quote {
                fn from(value: #item_ident_quote) -> Self {
                    let value: #spa_type_quote = value.into();
                    value
                }
            }
        };
        let from_variant_to_string_quote = self.variants.iter()
            .map(|(_, variant)| {
                let ident = variant.ident.to_token_stream();
                let ident_string = variant.ident.to_string();
                let attributes = variant.attributes.to_token_stream();
                quote! {
                    #attributes
                    Self::#ident => #ident_string.to_string(),
                }
            })
            .collect::<TokenStream>();
        let to_string_quote = quote! {
            #attributes_quote
            impl #item_ident_quote {
                fn to_string(&self) -> String {
                    match self {
                        #from_variant_to_string_quote
                    }
                }
            }
        };
        let items = vec![
            import_quote.to_string(),
            item_quote.to_string(),
            from_representation_type_quote.to_string(),
            to_representation_type_quote.to_string(),
            to_string_quote.to_string(),
        ];
        let items = items.join("\n");
        let file = syn::parse_file(items.as_str()).unwrap();
        prettyplease::unparse(&file)
    }
}