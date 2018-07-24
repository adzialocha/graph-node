use graphql_parser::query;
use graphql_parser::schema;
use hex;
use num_bigint;
use serde::{self, Deserialize, Serialize};

use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Display, Formatter};
use std::iter::FromIterator;
use std::ops::{Deref, DerefMut};
use std::str::FromStr;

/// An entity attribute name is represented as a string.
pub type Attribute = String;

pub const BYTES_SCALAR: &str = "Bytes";
pub const BIG_INT_SCALAR: &str = "BigInt";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct BigInt(num_bigint::BigInt);

impl BigInt {
    pub fn from_signed_bytes_le(bytes: &[u8]) -> Self {
        BigInt(num_bigint::BigInt::from_signed_bytes_le(bytes))
    }
}

impl Display for BigInt {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        self.0.fmt(f)
    }
}

impl FromStr for BigInt {
    type Err = <num_bigint::BigInt as FromStr>::Err;

    fn from_str(s: &str) -> Result<BigInt, Self::Err> {
        num_bigint::BigInt::from_str(s).map(|x| BigInt(x))
    }
}

impl Serialize for BigInt {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for BigInt {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;

        let decimal_string = String::deserialize(deserializer)?;
        BigInt::from_str(&decimal_string).map_err(D::Error::custom)
    }
}

/// An attribute value is represented as an enum with variants for all supported value types.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    String(String),
    Int(i32),
    Float(f32),
    Bool(bool),
    List(Vec<Value>),
    Null,
    /// In GraphQL, a hex string prefixed by `0x`.
    Bytes(Box<[u8]>),
    BigInt(BigInt),
}

impl Value {
    pub fn from_query_value(value: &query::Value, ty: &schema::Type) -> Value {
        use self::schema::Type::{ListType, NamedType};

        match (value, ty) {
            (query::Value::String(s), NamedType(n)) => {
                // Check if `ty` is a custom scalar type, otherwise assume it's
                // just a string.
                match n.as_str() {
                    BYTES_SCALAR => Value::Bytes(
                        hex::decode(s.trim_left_matches("0x"))
                            .expect("Value is not a hex string")
                            .into(),
                    ),
                    BIG_INT_SCALAR => {
                        Value::BigInt(BigInt::from_str(s).expect("Value is not a number"))
                    }
                    _ => Value::String(s.clone()),
                }
            }
            (query::Value::Int(i), _) => Value::Int(i.to_owned()
                .as_i64()
                .expect("Unable to parse graphql_parser::query::Number into i64")
                as i32),
            (query::Value::Float(f), _) => Value::Float(f.to_owned() as f32),
            (query::Value::Boolean(b), _) => Value::Bool(b.to_owned()),
            (query::Value::List(values), ListType(ty)) => Value::List(
                values
                    .iter()
                    .map(|value| Self::from_query_value(value, ty))
                    .collect(),
            ),
            (query::Value::Null, _) => Value::Null,
            _ => unimplemented!(),
        }
    }
}

impl From<Value> for query::Value {
    fn from(value: Value) -> Self {
        match value {
            Value::String(s) => query::Value::String(s.to_string()),
            Value::Int(i) => query::Value::Int(query::Number::from(i)),
            Value::Float(f) => query::Value::Float(f.into()),
            Value::Bool(b) => query::Value::Boolean(b),
            Value::Null => query::Value::Null,
            Value::List(values) => {
                query::Value::List(values.into_iter().map(|value| value.into()).collect())
            }
            Value::Bytes(bytes) => query::Value::String(format!("0x{}", hex::encode(bytes))),
            Value::BigInt(number) => query::Value::String(number.to_string()),
        }
    }
}

impl<'a> From<&'a str> for Value {
    fn from(value: &'a str) -> Value {
        Value::String(value.to_owned())
    }
}

impl From<String> for Value {
    fn from(value: String) -> Value {
        Value::String(value)
    }
}

impl<'a> From<&'a String> for Value {
    fn from(value: &'a String) -> Value {
        Value::String(value.clone())
    }
}

/// An entity is represented as a map of attribute names to values.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Entity(HashMap<Attribute, Value>);
impl Entity {
    /// Creates a new entity with no attributes set.
    pub fn new() -> Self {
        Entity(HashMap::new())
    }

    /// Merges an entity update `update` into this entity.
    ///
    /// If a key exists in both entities, the value from `update` is chosen.
    /// If a key only exists on one entity, the value from that entity is chosen.
    /// If a key is set to `Value::Null` in `update`, the key/value pair is removed.
    pub fn merge(&mut self, update: Entity) {
        for (key, value) in update.0.into_iter() {
            match value {
                Value::Null => self.remove(&key),
                _ => self.insert(key, value),
            };
        }
    }
}

impl Deref for Entity {
    type Target = HashMap<Attribute, Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Entity {
    fn deref_mut(&mut self) -> &mut HashMap<Attribute, Value> {
        &mut self.0
    }
}

impl Into<query::Value> for Entity {
    fn into(self) -> query::Value {
        let mut fields = BTreeMap::new();
        for (attr, value) in self.iter() {
            fields.insert(attr.to_string(), value.clone().into());
        }
        query::Value::Object(fields)
    }
}

impl From<HashMap<Attribute, Value>> for Entity {
    fn from(m: HashMap<Attribute, Value>) -> Entity {
        Entity(m)
    }
}

impl<'a> From<Vec<(&'a str, Value)>> for Entity {
    fn from(entries: Vec<(&'a str, Value)>) -> Entity {
        Entity::from(HashMap::from_iter(
            entries.into_iter().map(|(k, v)| (String::from(k), v)),
        ))
    }
}

#[test]
fn value_bytes() {
    let graphql_value = query::Value::String("0x8f494c66afc1d3f8ac1b45df21f02a46".to_owned());
    let ty = query::Type::NamedType(BYTES_SCALAR.to_owned());
    let from_query = Value::from_query_value(&graphql_value, &ty);
    assert_eq!(
        from_query,
        Value::Bytes(Box::new([
            143, 73, 76, 102, 175, 193, 211, 248, 172, 27, 69, 223, 33, 240, 42, 70
        ]))
    );
    assert_eq!(query::Value::from(from_query), graphql_value);
}

#[test]
fn value_bigint() {
    let big_num = "340282366920938463463374607431768211456";
    let graphql_value = query::Value::String(big_num.to_owned());
    let ty = query::Type::NamedType(BIG_INT_SCALAR.to_owned());
    let from_query = Value::from_query_value(&graphql_value, &ty);
    assert_eq!(
        from_query,
        Value::BigInt(BigInt::from_str(big_num).unwrap())
    );
    assert_eq!(query::Value::from(from_query), graphql_value);
}
