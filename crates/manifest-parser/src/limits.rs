#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    body_bytes: usize,
    header_value_bytes: usize,
    regex_pattern_bytes: usize,
    regex_size_bytes: usize,
    template_depth: usize,
    render_output_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            body_bytes: 2 * 1024 * 1024,
            header_value_bytes: 8 * 1024,
            regex_pattern_bytes: 1024,
            regex_size_bytes: 1024 * 1024,
            template_depth: 8,
            render_output_bytes: 8 * 1024,
        }
    }
}

impl Limits {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn body_bytes(&self) -> usize {
        self.body_bytes
    }

    #[must_use]
    pub fn with_body_bytes(mut self, bytes: usize) -> Self {
        self.body_bytes = self.body_bytes.min(bytes);
        self
    }

    #[must_use]
    pub fn header_value_bytes(&self) -> usize {
        self.header_value_bytes
    }

    #[must_use]
    pub fn with_header_value_bytes(mut self, bytes: usize) -> Self {
        self.header_value_bytes = self.header_value_bytes.min(bytes);
        self
    }

    #[must_use]
    pub fn regex_pattern_bytes(&self) -> usize {
        self.regex_pattern_bytes
    }

    #[must_use]
    pub fn with_regex_pattern_bytes(mut self, bytes: usize) -> Self {
        self.regex_pattern_bytes = self.regex_pattern_bytes.min(bytes);
        self
    }

    #[must_use]
    pub fn regex_size_bytes(&self) -> usize {
        self.regex_size_bytes
    }

    #[must_use]
    pub fn with_regex_size_bytes(mut self, bytes: usize) -> Self {
        self.regex_size_bytes = self.regex_size_bytes.min(bytes);
        self
    }

    #[must_use]
    pub fn template_depth(&self) -> usize {
        self.template_depth
    }

    #[must_use]
    pub fn with_template_depth(mut self, depth: usize) -> Self {
        self.template_depth = self.template_depth.min(depth);
        self
    }

    #[must_use]
    pub fn render_output_bytes(&self) -> usize {
        self.render_output_bytes
    }

    #[must_use]
    pub fn with_render_output_bytes(mut self, bytes: usize) -> Self {
        self.render_output_bytes = self.render_output_bytes.min(bytes);
        self
    }
}
