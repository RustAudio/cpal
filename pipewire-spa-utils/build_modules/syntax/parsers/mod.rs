use syn::{Attribute, Fields, ImplItem, ImplItemConst, ItemImpl, ItemStruct, Path, Type};

pub struct StructVisitor<'a> {
    item: &'a ItemStruct,
}

impl<'a> StructVisitor<'a> {
    pub fn new(item: &'a ItemStruct) -> Self {
        Self {
            item,
        }
    }

    pub fn fields(&self) -> Fields {
        self.item.fields.clone()
    }
}

pub struct StructImplVisitor<'a> {
    item: &'a ItemImpl
}

impl<'a> StructImplVisitor<'a> {
    pub fn new(item: &'a ItemImpl) -> Self {
        Self {
            item,
        }
    }
    pub fn self_type(&self) -> Path {
        match *self.item.self_ty.clone() {
            Type::Path(value) => {
                value.path.clone()
            }
            _ => panic!("Path expected")
        }
    }

    pub fn attributes(&self) -> Vec<Attribute> {
        self.item.attrs.iter()
            .map(move |attribute| {
                attribute.clone()
            })
            .collect::<Vec<_>>()
    }

    pub fn constants(&self) -> Vec<ImplItemConst> {
        self.item.items.iter()
            .filter_map(move |item| {
                match item {
                    ImplItem::Const(value) => {
                        Some(value.clone())
                    }
                    &_ => return None
                }
            })
            .collect::<Vec<_>>()
    }
}

