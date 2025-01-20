use ::impl_any_deserializer;
use libspa::pod::deserialize::DeserializeError;
use libspa::pod::deserialize::DeserializeSuccess;
use libspa::pod::deserialize::PodDeserialize;
use libspa::pod::deserialize::PodDeserializer;
use libspa::pod::deserialize::{ChoiceIdVisitor, ChoiceIntVisitor};
use libspa::pod::{ChoiceValue, Value};
use libspa::utils::{Choice, ChoiceEnum, Id};
use std::ops::Deref;
use impl_choice_int_deserializer;

#[derive(Debug, Clone)]
pub struct IntOrChoiceInt(u32);

impl From<u32> for IntOrChoiceInt {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<ChoiceValue> for IntOrChoiceInt {
    fn from(value: ChoiceValue) -> Self {
        match value {
            ChoiceValue::Int(value) => value.into(),
            _ => panic!("Expected Int or ChoiceValue::Int"),
        }
    }
}

impl From<Choice<i32>> for IntOrChoiceInt {
    fn from(value: Choice<i32>) -> Self {
        match value.1 {
            ChoiceEnum::None(value) => IntOrChoiceInt(value as u32),
            _ => panic!("Expected ChoiceEnum::None"),
        }
    }
}

impl From<Value> for IntOrChoiceInt {
    fn from(value: Value) -> Self {
        match value {
            Value::Int(value) => Self(value as u32),
            Value::Choice(value) => value.into(),
            _ => panic!("Expected Int or Choice")
        }
    }
}

impl Deref for IntOrChoiceInt {
    type Target = u32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl_any_deserializer!(IntOrChoiceInt);

#[derive(Debug, Clone)]
pub struct RangeInt32 {
    pub value: u32,
    pub minimum: u32,
    pub maximum: u32,
}

impl RangeInt32 {
    fn new(value: u32, minimum: u32, maximum: u32) -> Self {
        Self {
            value,
            minimum,
            maximum,
        }
    }
}

impl From<ChoiceEnum<i32>> for RangeInt32 {
    fn from(value: ChoiceEnum<i32>) -> Self {
        match value {
            ChoiceEnum::Range {
                default, min, max
            } => RangeInt32::new(
                default as u32, min as u32, max as u32,
            ),
            _ => panic!("Expected ChoiceEnum<i32>::Range")
        }
    }
}

impl_choice_int_deserializer!(RangeInt32);

#[derive(Debug, Clone)]
pub struct IntOrRangeInt32(RangeInt32);

impl From<u32> for IntOrRangeInt32 {
    fn from(value: u32) -> Self {
        Self(RangeInt32::new(value, value, value))
    }
}

impl From<i32> for IntOrRangeInt32 {
    fn from(value: i32) -> Self {
        Self(RangeInt32::new(value as u32, value as u32, value as u32))
    }
}

impl From<ChoiceValue> for IntOrRangeInt32 {
    fn from(value: ChoiceValue) -> Self {
        match value {
            ChoiceValue::Int(value) => value.into(),
            _ => panic!("Expected ChoiceValue::Int")
        }
    }
}

impl From<Choice<i32>> for IntOrRangeInt32 {

    fn from(value: Choice<i32>) -> Self {
        match value.1 {
            ChoiceEnum::None(value) => {
                Self(RangeInt32::new(value as u32, value as u32, value as u32))
            }
            ChoiceEnum::Range { default, min, max } => {
                Self(RangeInt32::new(default as u32, min as u32, max as u32))
            }
            _ => panic!("Expected Choice<i32>::None or Choice<i32>::Range")
        }
    }
}

impl From<Value> for IntOrRangeInt32 {

    fn from(value: Value) -> Self {
        match value {
            Value::Int(value) => Self::from(value),
            Value::Choice(value) => value.into(),
            _ => panic!("Expected Int or Choice")
        }
    }
}

impl Deref for IntOrRangeInt32 {
    type Target = RangeInt32;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl_any_deserializer!(IntOrRangeInt32);

#[derive(Debug, Clone)]
pub struct EnumId<T> {
    pub default: T,
    pub alternatives: Vec<T>,
}

impl <T: Ord> EnumId<T> {
    fn new(default: T, mut alternatives: Vec<T>) -> Self {
        alternatives.sort_by(move |a, b| {
            a.cmp(b)
        });
        Self {
            default,
            alternatives,
        }
    }
}

impl <T: From<u32> + Ord> From<ChoiceEnum<Id>> for EnumId<T> {
    fn from(value: ChoiceEnum<Id>) -> Self {
        match value {
            ChoiceEnum::Enum {
                default, alternatives
            } => EnumId::new(
                default.0.into(),
                alternatives.into_iter()
                    .map(move |id| id.0.into())
                    .collect(),
            ),
            _ => panic!("Expected ChoiceEnum<Id>::Enum")
        }
    }
}

impl <T: From<u32> + Ord> From<Choice<Id>> for EnumId<T> {
    fn from(value: Choice<Id>) -> Self {
        value.1.into()
    }
}

impl <'de, T: From<u32> + Ord> PodDeserialize<'de> for EnumId<T> {
    fn deserialize(deserializer: PodDeserializer<'de>) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
    where
        Self: Sized
    {
        let res = deserializer.deserialize_choice(ChoiceIdVisitor)?;
        Ok((res.0.into(), res.1))
    }
}

#[derive(Debug, Clone)]
pub struct IdOrEnumId<T>(EnumId<T>);

impl <T: From<u32> + Ord> From<ChoiceValue> for IdOrEnumId<T> {
    fn from(value: ChoiceValue) -> Self {
        match value {
            ChoiceValue::Id(value) => value.into(),
            _ => panic!("Expected ChoiceValue::Id")
        }
    }
}

impl <T: From<u32> + Ord> From<Choice<Id>> for IdOrEnumId<T> {
    fn from(value: Choice<Id>) -> Self {
        match value.1 {
            ChoiceEnum::Enum { default, alternatives } => {
                Self(EnumId::new(
                    default.0.into(),
                    alternatives.into_iter()
                        .map(move |id| id.0.into())
                        .collect::<Vec<T>>()
                ))
            }
            _ => panic!("Expected Choice<Id>::Enum")
        }
    }
}

impl <T: From<u32> + Ord> From<Value> for IdOrEnumId<T> {
    fn from(value: Value) -> Self {
        match value {
            Value::Id(value) => Self(EnumId::new(value.0.into(), vec![value.0.into()])),
            Value::Choice(value) => value.into(),
            _ => panic!("Expected Id or Choice")
        }
    }
}

impl <T> Deref for IdOrEnumId<T> {
    type Target = EnumId<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl <'de, T: From<u32> + Ord> PodDeserialize<'de> for IdOrEnumId<T> {
    fn deserialize(deserializer: PodDeserializer<'de>) -> Result<(Self, DeserializeSuccess<'de>), DeserializeError<&'de [u8]>>
    where
        Self: Sized
    {
        let res = deserializer.deserialize_any()?;
        Ok((res.0.into(), res.1))
    }
}