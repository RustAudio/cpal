use build_modules::syntax::generators::enumerator::{EnumInfo, EnumVariantInfo};
use build_modules::syntax::parsers::{StructImplVisitor, StructVisitor};
use build_modules::syntax::utils::AttributeExt;
use build_modules::utils::read_source_file;
use indexmap::IndexMap;
use itertools::Itertools;
use std::cmp::Ordering;
use std::path::PathBuf;
use syn::__private::quote::__private::ext::RepToTokensExt;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::token::PathSep;
use syn::{Attribute, Expr, Fields, Ident, ImplItemConst, Item, ItemConst, PathSegment, Type};
use debug;

#[derive(Debug, Clone)]
struct StructInfo {
    ident: Ident,
    unnamed_field_ident: Ident,
}

#[derive(Debug, Clone)]
struct StructImplInfo {
    attributes: Vec<Attribute>,
    constants: Vec<ImplItemConst>
}

pub fn generate_enums(src_path: &PathBuf, build_path: &PathBuf, features: &Vec<String>) {
    let file_path = PathBuf::from(&"param/format.rs");
    let src = read_source_file(&src_path, &file_path);

    let media_type_enum_info = map_media_type_enum_info(&src.items);
    let media_subtype_enum_info = map_media_subtype_enum_info(
        &src.items,
        move |constant | {
            if features.is_empty() {
                constant.attrs.contains(&"feature".to_string()) == false
            }
            else {
                features.iter().any(|feature| {
                    constant.attrs.contains(feature)
                }) == false
            }
        }
    );

    let enum_infos = vec![
        media_type_enum_info,
        media_subtype_enum_info
    ];

    generate_enums_code(enum_infos, "format.rs");

    let file_path = PathBuf::from(&"bindings.rs");
    let src = read_source_file(&build_path, &file_path);

    let audio_sample_format_enum_info = map_audio_sample_format_enum_info(&src.items);
    let audio_channel_enum_info = map_audio_channel_enum_info(&src.items);

    let enum_infos = vec![
        audio_sample_format_enum_info,
        audio_channel_enum_info
    ];

    generate_enums_code(enum_infos, "audio.rs");
}

fn map_media_type_enum_info(items: &Vec<Item>) -> EnumInfo {
    const IDENT: &str = "MediaType";

    let filter = move |ident: String| ident == IDENT;
    let struct_info = map_struct_info(&items, filter);
    let struct_impl_info = map_struct_impl_info(&items, filter);
    
    EnumInfo {
        ident: struct_info.ident.clone(),
        attributes: struct_impl_info.attributes,
        spa_type: struct_info.unnamed_field_ident.clone(),
        representation_type: "u32".to_string(),
        variants: struct_impl_info.constants.iter()
                .map(move |constant| {
                    let index = constant.ident.to_string();
                    let variant = EnumVariantInfo {
                        attributes: constant.attrs.clone(),
                        fields: Fields::Unit,
                        ident: constant.ident.clone(),
                        discriminant: match constant.expr.clone() {
                            Expr::Call(value) => {
                                let mut arg = value.args[0].clone();
                                match arg {
                                    Expr::Path(ref mut value) => {
                                        let mut segments = Punctuated::<PathSegment, PathSep>::new();
                                        for index in 1..value.path.segments.len() {
                                            segments.push(value.path.segments[index].clone());
                                        }
                                        value.path.segments = segments;
                                    },
                                    _ => panic!("Expected a path expression"),
                                };
                                arg
                            },
                            _ => panic!("Expected a call expression"),
                        },
                    };
                    (index, variant)
                })
                .collect::<IndexMap<_, _>>(),
    }
}

fn map_media_subtype_enum_info<F>(items: &Vec<Item>, filter: F) -> EnumInfo
where
    F: FnMut(&&ImplItemConst) -> bool
{
    const IDENT: &str = "MediaSubtype";

    let ident_filter = move |ident: String| ident == IDENT;
    let struct_info = map_struct_info(&items, ident_filter);
    let mut struct_impl_info = map_struct_impl_info(&items, ident_filter);
    
    struct_impl_info.attributes.push_one("allow", "unexpected_cfgs"); // TODO remove this when V0_3_68 will be added to libspa manifest
    struct_impl_info.attributes.push_one("allow", "unused_doc_comments");

    EnumInfo {
        ident: struct_info.ident.clone(),
        attributes: struct_impl_info.attributes,
        spa_type: struct_info.unnamed_field_ident.clone(),
        representation_type: "u32".to_string(),
        variants: struct_impl_info.constants.iter()
            .filter(filter)
            .filter(move |constant| {
                match &constant.expr {
                    Expr::Call(_) => true,
                    _ => false,
                }
            })
            .map(move |constant| {
                let index = constant.ident.to_string();
                let variant = EnumVariantInfo {
                    attributes: constant.attrs.clone(),
                    fields: Fields::Unit,
                    ident: constant.ident.clone(),
                    discriminant: match constant.expr.clone() {
                        Expr::Call(value) => {
                            let mut arg = value.args[0].clone();
                            match arg {
                                Expr::Path(ref mut value) => {
                                    let mut segments = Punctuated::<PathSegment, PathSep>::new();
                                    for index in 1..value.path.segments.len() {
                                        segments.push(value.path.segments[index].clone());
                                    }
                                    value.path.segments = segments;
                                },
                                _ => panic!("Expected a path expression"),
                            };
                            arg
                        },
                        _ => panic!("Expected a call expression: {:?}", constant.expr),
                    },
                };
                (index, variant)
            })
            .collect::<IndexMap<_, _>>(),
    }
}

fn spa_audio_format_idents() -> Vec<String> {
    let audio_formats: Vec<String> = vec![
        "S8".to_string(),
        "U8".to_string(),
        "S16".to_string(),
        "U16".to_string(),
        "S24".to_string(),
        "U24".to_string(),
        "S24_32".to_string(),
        "U24_32".to_string(),
        "S32".to_string(),
        "U32".to_string(),
        "F32".to_string(),
        "F64".to_string(),
    ];

    let ends = vec![
        "LE".to_string(),
        "BE".to_string(),
        "P".to_string(),
    ];

    audio_formats.iter()
        .flat_map(move |format| {
            ends.iter()
                .map(move |end| {
                    if format.contains("8") && end != "P" {
                        format!("SPA_AUDIO_FORMAT_{}", format)
                    }
                    else if format.contains("8") && end == "P" {
                        format!("SPA_AUDIO_FORMAT_{}{}", format, end)
                    }
                    else if format.contains("8") == false && end == "P" {
                        format!("SPA_AUDIO_FORMAT_{}{}", format, end)
                    }
                    else {
                        format!("SPA_AUDIO_FORMAT_{}_{}", format, end)
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn map_audio_sample_format_enum_info(items: &Vec<Item>) -> EnumInfo {
    let spa_audio_format_idents = spa_audio_format_idents();
    let constants = map_constant_info(
        &items,
        move |constant| spa_audio_format_idents.contains(constant),
        move |a, b| {
            a.cmp(&b)
        }
    );

    let ident = "AudioSampleFormat";
    let spa_type = "spa_audio_format";
    
    let mut attributes: Vec<Attribute> = vec![];
    attributes.push_one("allow", "non_camel_case_types");

    EnumInfo {
        ident: Ident::new(ident, ident.span()),
        attributes,
        spa_type: Ident::new(spa_type, spa_type.span()),
        representation_type: "u32".to_string(),
        variants: constants.iter()
            .map(move |constant| {
                let index = constant.ident.to_string();
                let ident = constant.ident.to_string().replace("SPA_AUDIO_FORMAT_", "");
                let ident = Ident::new(&ident, ident.span());
                let discriminant = *constant.expr.clone();
                let variant = EnumVariantInfo {
                    attributes: constant.attrs.clone(),
                    fields: Fields::Unit,
                    ident,
                    discriminant,
                };
                (index, variant)
            })
            .collect::<IndexMap<_, _>>(),
    }
}

fn map_audio_channel_enum_info(items: &Vec<Item>) -> EnumInfo {
    let constants = map_constant_info(
        &items,
        move |constant| {
            if constant.starts_with("SPA_AUDIO_CHANNEL") == false {
                return false;
            }
            
            let constant = constant.replace("SPA_AUDIO_CHANNEL_", "");
            
            if constant.starts_with("START") || constant.starts_with("LAST") || constant.starts_with("AUX") {
                return false;
            }
            
            return true;
        },
        move |a, b| {
            a.cmp(&b)
        }
    );

    let ident = "AudioChannel";
    let spa_type = "spa_audio_channel";

    let mut attributes: Vec<Attribute> = vec![];
    attributes.push_one("allow", "unused_doc_comments");

    EnumInfo {
        ident: Ident::new(ident, ident.span()),
        attributes,
        spa_type: Ident::new(spa_type, spa_type.span()),
        representation_type: "u32".to_string(),
        variants: constants.iter()
            .map(move |constant| {
                let index = constant.ident.to_string();
                let ident = constant.ident.to_string().replace("SPA_AUDIO_CHANNEL_", "");
                let ident = Ident::new(&ident, ident.span());
                let discriminant = *constant.expr.clone();
                let variant = EnumVariantInfo {
                    attributes: constant.attrs.clone(),
                    fields: Fields::Unit,
                    ident,
                    discriminant,
                };
                (index, variant)
            })
            .collect::<IndexMap<_, _>>(),
    }
}

fn map_constant_info<F, S>(items: &Vec<Item>, filter: F, sorter: S) -> Vec<&ItemConst>
where
    F: Fn(&String) -> bool,
    S: Fn(&String, &String) -> Ordering
{
    items.iter()
        .filter_map(move |item| {
            match item {
                Item::Const(value) => {
                    Some(value)
                }
                &_ => None
            }
        })
        .filter(move |constant| filter(&constant.ident.to_string()))
        .sorted_by(move |a, b| sorter(&a.ident.to_string(), &b.ident.to_string()))
        .collect::<Vec<_>>()
}

fn map_struct_info<F>(items: &Vec<Item>, filter: F) -> StructInfo
where
    F: Fn(String) -> bool
{
    items.iter()
        .filter_map(move |item| {
            let item = item.next().unwrap();
            let item = item.next().unwrap();
            match item {
                Item::Struct(value) => {
                    let ident = value.ident.clone();
                    if filter(ident.to_string()) == false {
                        return None;
                    }
                    let visitor = StructVisitor::new(value);
                    Some((visitor, ident))
                }
                &_ => None
            }
        })
        .filter_map(move |(visitor, ident)| {
            let fields = visitor.fields();
            if fields.is_empty() == false {
                Some(StructInfo {
                    ident,
                    unnamed_field_ident: {
                        let field = fields
                            .iter()
                            .map(|field| field.clone())
                            .collect::<Vec<_>>()
                            .first()
                            .cloned()
                            .unwrap();
                        let ident = match field.ty {
                            Type::Path(value) => {
                                value.path.segments.iter()
                                    .map(|segment| segment.ident.to_string())
                                    .join("::")
                            }
                            _ => panic!("Unsupported type: {:?}", field.ty),
                        };
                        let ident = ident
                            .replace("spa_sys::", "");
                        Ident::new(&ident, ident.span())
                    },
                })
            }
            else {
                None
            }
        })
        .collect::<Vec<_>>()
        .first()
        .cloned()
        .unwrap()
}

fn map_struct_impl_info<F>(items: &Vec<Item>, filter: F) -> StructImplInfo
where
    F: Fn(String) -> bool
{
    items.iter()
        .filter_map(move |item| {
            let item = item.next().unwrap();
            let item = item.next().unwrap();
            match item {
                Item::Impl(value) => {
                    let visitor = StructImplVisitor::new(value);
                    let self_ident = visitor.self_type()
                        .segments
                        .iter()
                        .map(|segment| segment.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::");
                    if filter(self_ident) == false {
                        return None;
                    }
                    let attributes = visitor.attributes();
                    Some((visitor, attributes))
                }
                &_ => None
            }
        })
        .filter_map(move |(visitor, attributes)| {
            if attributes.is_empty() {
                return None;
            }
            let constants = visitor.constants()
                .iter()
                .filter_map(move |constant| {
                    match constant.ty {
                        Type::Path(_) => {
                            Some(constant.clone())
                        }
                        _ => None
                    }
                })
                .collect::<Vec<_>>();
            if constants.is_empty() == false {
                Some(StructImplInfo {
                    attributes,
                    constants: constants.clone(),
                })
            }
            else {
                None
            }
        })
        .collect::<Vec<_>>()
        .first()
        .cloned()
        .unwrap()
}

fn generate_enums_code(enums: Vec<EnumInfo>, filename: &str) {
    let code = enums.iter()
        .map(move |enum_info| enum_info.generate())
        .collect::<Vec<_>>()
        .join("\n");

    let out_dir = std::env::var("OUT_DIR")
        .expect("OUT_DIR not set");

    let path = std::path::Path::new(&out_dir).join(filename);
    std::fs::write(path, code)
        .expect("Unable to write generated file");
}