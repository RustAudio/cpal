#[macro_export]
macro_rules! impl_id_deserializer {
    (
        $name:ident
    ) => {        
        impl From<Id> for $name {
            fn from(value: Id) -> Self {
                value.0.into()
            }
        }

        impl<'de> PodDeserialize<'de> for $name {
            fn deserialize(deserializer: PodDeserializer<'de>) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
            where
                Self: Sized
            {
                let res = deserializer.deserialize_id(IdVisitor)?;
                Ok((res.0.into(), res.1))
            }
        }
    }
}

#[macro_export]
macro_rules! impl_choice_id_deserializer {
    (
        $name:ident
    ) => {
        impl From<Choice<Id>> for $name {
            fn from(value: Choice<Id>) -> Self {
                value.1.into()
            }
        }

        impl<'de> PodDeserialize<'de> for $name {
            fn deserialize(deserializer: PodDeserializer<'de>) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
            where
                Self: Sized
            {
                let res = deserializer.deserialize_choice(ChoiceIdVisitor)?;
                Ok((res.0.into(), res.1))
            }
        }
    }
}

#[macro_export]
macro_rules! impl_choice_int_deserializer {
    (
        $name:ident
    ) => {
        impl From<Choice<i32>> for $name {
            fn from(value: Choice<i32>) -> Self {
                value.1.into()
            }
        }

        impl<'de> PodDeserialize<'de> for $name {
            fn deserialize(deserializer: PodDeserializer<'de>) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
            where
                Self: Sized
            {
                let res = deserializer.deserialize_choice(ChoiceIntVisitor)?;
                Ok((res.0.into(), res.1))
            }
        }
    }
}

#[macro_export]
macro_rules! impl_array_id_deserializer {
    (
        $array_name:ident,
        $item_name:ident
    ) => {
        impl From<&Id> for $item_name {
            fn from(value: &Id) -> Self {
                value.0.into()
            }
        }

        impl From<Vec<Id>> for $array_name {
            fn from(value: Vec<Id>) -> Self {
                $array_name(value.iter().map(|id| id.into()).collect())
            }
        }

        impl<'de> PodDeserialize<'de> for $array_name {
            fn deserialize(deserializer: PodDeserializer<'de>) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
            where
                Self: Sized
            {
                let res = deserializer.deserialize_array(VecVisitor::default())?;
                Ok((res.0.into(), res.1))
            }
        }
    }
}

#[macro_export]
macro_rules! impl_any_deserializer {
    (
        $name:ident
    ) => {
        impl<'de> PodDeserialize<'de> for $name {
            fn deserialize(deserializer: PodDeserializer<'de>) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
            where
                Self: Sized
            {
                let res = deserializer.deserialize_any()?;
                Ok((res.0.into(), res.1))
            }
        }
    }
}