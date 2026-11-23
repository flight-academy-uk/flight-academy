//! Public identifiers per ADR-006 §D.
//!
//! Each resource has its own newtype over a UUID, so a tenant ID cannot be
//! passed where a user ID is expected. On the wire an ID is a type prefix,
//! an underscore, and the UUID as 26 characters of Crockford base32:
//!
//! ```text
//! ten_1Z3JD552VY9MGSR1BE59XM1P8K   (UUID 3f1c9a52-8b7e-4d21-9c05-6e2a7b40d913)
//! ```
//!
//! The encoding is reversible, so the database keeps plain `uuid` columns
//! and nothing maps one form to the other. The prefix is a type tag, not a
//! secret: anyone can recover the UUID. Entity UUIDs are therefore random
//! (v4); a v7 UUID would publish when the record was created.
//!
//! Parsing accepts only the canonical form this module writes: the exact
//! prefix, then 26 upper-case characters whose first is `0` to `7` (128 bits
//! need 26 five-bit characters, leaving the top two bits zero). Lower-case
//! input and Crockford's look-alike substitutions (`I`, `L`, `O`) are
//! refused, so every ID has exactly one spelling.

use core::fmt;
use core::str::FromStr;

use uuid::Uuid;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const ENCODED_LEN: usize = 26;

fn encode(uuid: Uuid) -> [u8; ENCODED_LEN] {
    let value = uuid.as_u128();
    let mut out = [0u8; ENCODED_LEN];
    for (i, slot) in out.iter_mut().enumerate() {
        let shift = 5 * (ENCODED_LEN - 1 - i);
        *slot = ALPHABET[((value >> shift) & 0x1f) as usize];
    }
    out
}

fn decode(encoded: &str) -> Option<Uuid> {
    let bytes = encoded.as_bytes();
    if bytes.len() != ENCODED_LEN || bytes[0] > b'7' {
        return None;
    }
    let mut value: u128 = 0;
    for &byte in bytes {
        let digit = ALPHABET.iter().position(|&c| c == byte)?;
        value = (value << 5) | digit as u128;
    }
    Some(Uuid::from_u128(value))
}

/// A string that is not a well-formed ID of the expected type. The HTTP
/// layer reports it as `400`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdParseError {
    pub expected_prefix: &'static str,
}

impl fmt::Display for IdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "expected `{}_` followed by {ENCODED_LEN} Crockford base32 characters",
            self.expected_prefix
        )
    }
}

impl std::error::Error for IdParseError {}

macro_rules! prefixed_id {
    ($(#[$meta:meta])* $name:ident, $prefix:literal, $pattern:literal) => {
        $(#[$meta])*
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name {
            uuid: Uuid,
        }

        impl $name {
            /// The wire prefix, without the underscore.
            pub const PREFIX: &'static str = $prefix;

            /// A new random (v4) ID.
            pub fn new() -> Self {
                Self { uuid: Uuid::new_v4() }
            }

            pub const fn from_uuid(uuid: Uuid) -> Self {
                Self { uuid }
            }

            pub const fn as_uuid(&self) -> Uuid {
                self.uuid
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<Uuid> for $name {
            fn from(uuid: Uuid) -> Self {
                Self { uuid }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let encoded = encode(self.uuid);
                let encoded = core::str::from_utf8(&encoded).expect("alphabet is ASCII");
                write!(f, "{}_{encoded}", Self::PREFIX)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({self})", stringify!($name))
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                s.strip_prefix(Self::PREFIX)
                    .and_then(|rest| rest.strip_prefix('_'))
                    .and_then(decode)
                    .map(Self::from_uuid)
                    .ok_or(IdParseError {
                        expected_prefix: Self::PREFIX,
                    })
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let s = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }

        #[cfg(feature = "sqlx")]
        impl sqlx::Type<sqlx::Postgres> for $name {
            fn type_info() -> sqlx::postgres::PgTypeInfo {
                <Uuid as sqlx::Type<sqlx::Postgres>>::type_info()
            }
        }

        #[cfg(feature = "sqlx")]
        impl sqlx::Encode<'_, sqlx::Postgres> for $name {
            fn encode_by_ref(
                &self,
                buf: &mut sqlx::postgres::PgArgumentBuffer,
            ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
                <Uuid as sqlx::Encode<sqlx::Postgres>>::encode_by_ref(&self.uuid, buf)
            }
        }

        #[cfg(feature = "sqlx")]
        impl sqlx::Decode<'_, sqlx::Postgres> for $name {
            fn decode(value: sqlx::postgres::PgValueRef<'_>) -> Result<Self, sqlx::error::BoxDynError> {
                <Uuid as sqlx::Decode<sqlx::Postgres>>::decode(value).map(Self::from_uuid)
            }
        }

        #[cfg(feature = "openapi")]
        impl utoipa::PartialSchema for $name {
            fn schema() -> utoipa::openapi::RefOr<utoipa::openapi::schema::Schema> {
                utoipa::openapi::schema::ObjectBuilder::new()
                    .schema_type(utoipa::openapi::schema::Type::String)
                    .pattern(Some($pattern))
                    .into()
            }
        }

        #[cfg(feature = "openapi")]
        impl utoipa::ToSchema for $name {}
    };
}

prefixed_id!(
    /// Identifies a tenant (an organisation such as a flight school).
    TenantId,
    "ten",
    "^ten_[0-7][0-9A-HJKMNP-TV-Z]{25}$"
);

prefixed_id!(
    /// Identifies a user account.
    UserId,
    "usr",
    "^usr_[0-7][0-9A-HJKMNP-TV-Z]{25}$"
);

#[cfg(test)]
mod tests {
    use super::*;

    const ADR_EXAMPLE_UUID: &str = "3f1c9a52-8b7e-4d21-9c05-6e2a7b40d913";
    const ADR_EXAMPLE_ID: &str = "ten_1Z3JD552VY9MGSR1BE59XM1P8K";

    fn example() -> TenantId {
        TenantId::from_uuid(ADR_EXAMPLE_UUID.parse().unwrap())
    }

    #[test]
    fn encodes_the_adr_example() {
        assert_eq!(example().to_string(), ADR_EXAMPLE_ID);
        assert_eq!(ADR_EXAMPLE_ID.parse::<TenantId>().unwrap(), example());
    }

    #[test]
    fn round_trips_the_extremes() {
        for uuid in [Uuid::nil(), Uuid::max()] {
            let id = UserId::from_uuid(uuid);
            assert_eq!(id.to_string().parse::<UserId>().unwrap(), id);
        }
        assert_eq!(
            UserId::from_uuid(Uuid::nil()).to_string(),
            "usr_00000000000000000000000000"
        );
        assert_eq!(
            UserId::from_uuid(Uuid::max()).to_string(),
            "usr_7ZZZZZZZZZZZZZZZZZZZZZZZZZ"
        );
    }

    #[test]
    fn round_trips_random_ids() {
        for _ in 0..1000 {
            let id = TenantId::new();
            assert_eq!(id.to_string().len(), 4 + ENCODED_LEN);
            assert_eq!(id.to_string().parse::<TenantId>().unwrap(), id);
        }
    }

    #[test]
    fn new_ids_are_v4() {
        assert_eq!(TenantId::new().as_uuid().get_version_num(), 4);
    }

    #[test]
    fn refuses_anything_but_the_canonical_form() {
        let refused = [
            "usr_1Z3JD552VY9MGSR1BE59XM1P8K",  // another type's prefix
            "1Z3JD552VY9MGSR1BE59XM1P8K",      // no prefix
            "ten1Z3JD552VY9MGSR1BE59XM1P8K",   // no underscore
            "ten_1z3jd552vy9mgsr1be59xm1p8k",  // lower case
            "ten_1Z3JD552VY9MGSR1BE59XM1P8",   // too short
            "ten_1Z3JD552VY9MGSR1BE59XM1P8KK", // too long
            "ten_8Z3JD552VY9MGSR1BE59XM1P8K",  // more than 128 bits
            "ten_1Z3JD552VY9MGSR1BE59XM1P8I",  // I is not in the alphabet
            "ten_1Z3JD552VY9MGSR1BE59XM1P8U",  // U is not in the alphabet
            "ten_",
            "",
            ADR_EXAMPLE_UUID,
        ];
        for input in refused {
            assert_eq!(
                input.parse::<TenantId>(),
                Err(IdParseError {
                    expected_prefix: "ten"
                }),
                "{input:?} should be refused"
            );
        }
    }

    #[test]
    fn serde_uses_the_wire_form() {
        let json = serde_json::to_string(&example()).unwrap();
        assert_eq!(json, format!("\"{ADR_EXAMPLE_ID}\""));
        assert_eq!(serde_json::from_str::<TenantId>(&json).unwrap(), example());
        let err =
            serde_json::from_str::<TenantId>("\"usr_1Z3JD552VY9MGSR1BE59XM1P8K\"").unwrap_err();
        assert!(err.to_string().contains("expected `ten_`"), "{err}");
    }

    #[test]
    fn debug_names_the_type() {
        assert_eq!(
            format!("{:?}", example()),
            format!("TenantId({ADR_EXAMPLE_ID})")
        );
    }
}
