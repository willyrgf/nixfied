//! A list whose elements are required to be distinct. `Deserialize` rejects a
//! duplicate at the wire boundary, so a model that lists the same service,
//! binding, or exit code twice is *inexpressible* rather than admitted-then-
//! mis-run. The wire shape is an ordinary JSON array (`#[serde(transparent)]` on
//! serialize), so this carries no schema change.

use serde::{Deserialize, Deserializer, Serialize};

/// An ordered list of distinct elements. Order is preserved (it is meaningful for
/// e.g. service start order); only multiplicity is constrained.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct UniqueVec<T>(Vec<T>);

impl<T> Default for UniqueVec<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> UniqueVec<T> {
    /// Borrow the elements as a slice.
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }

    /// Iterate the elements in declaration order.
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }

    /// Number of elements.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a, T> IntoIterator for &'a UniqueVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de, T> Deserialize<'de> for UniqueVec<T>
where
    T: Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let items = Vec::<T>::deserialize(deserializer)?;
        // The lists this guards are tiny (services, bindings, exit codes), so the
        // quadratic distinctness scan is irrelevant and needs no `Hash`/`Ord` on T.
        for (index, element) in items.iter().enumerate() {
            if items[..index].contains(element) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate element {element:?}"
                )));
            }
        }
        Ok(Self(items))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_distinct_elements() {
        let parsed: UniqueVec<String> =
            serde_json::from_str(r#"["a","b","c"]"#).expect("distinct elements parse");
        assert_eq!(parsed.len(), 3);
    }

    #[test]
    fn rejects_duplicate_elements() {
        let error = serde_json::from_str::<UniqueVec<String>>(r#"["a","b","a"]"#)
            .expect_err("a duplicate must not deserialize");
        assert!(error.to_string().contains("duplicate element"));
    }

    #[test]
    fn rejects_duplicate_integers() {
        assert!(serde_json::from_str::<UniqueVec<i32>>("[0,1,0]").is_err());
    }

    #[test]
    fn round_trips_to_a_plain_array() {
        let parsed: UniqueVec<String> = serde_json::from_str(r#"["x","y"]"#).unwrap();
        assert_eq!(serde_json::to_string(&parsed).unwrap(), r#"["x","y"]"#);
    }
}
