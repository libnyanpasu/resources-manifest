use crate::ParseError;

#[derive(Debug, Clone, Copy)]
pub struct RemoteResponse<'a> {
    pub body: &'a str,
    pub headers: &'a [(String, String)],
}

impl<'response> RemoteResponse<'response> {
    #[must_use = "header values must be consumed"]
    pub fn header_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'response str> + 'a {
        self.headers
            .iter()
            .filter(move |(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    #[must_use = "header lookup errors must be handled"]
    pub fn single_header(&self, name: &str) -> Result<&'response str, ParseError> {
        let mut values = self.header_values(name);
        let value = values.next().ok_or_else(|| ParseError::FindHeader {
            name: name.to_owned(),
        })?;
        if values.any(|other| other != value) {
            return Err(ParseError::SelectHeader {
                name: name.to_owned(),
            });
        }
        Ok(value)
    }
}
