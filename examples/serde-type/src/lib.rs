pub struct Counter(pub i128);

impl serde::Serialize for Counter {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        newer_consumer::touch();

        serializer.serialize_i128(self.0)
    }
}
