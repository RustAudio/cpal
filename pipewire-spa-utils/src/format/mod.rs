use libspa::utils::Id;
use libspa::pod::deserialize::DeserializeError;
use libspa::pod::deserialize::DeserializeSuccess;
use libspa::pod::deserialize::PodDeserialize;
use libspa::pod::deserialize::PodDeserializer;
use libspa::pod::deserialize::IdVisitor;
use ::{impl_id_deserializer};

include!(concat!(env!("OUT_DIR"), "/format.rs"));

impl_id_deserializer!(MediaType);
impl_id_deserializer!(MediaSubtype);