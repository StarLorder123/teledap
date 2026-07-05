//! Recursive variable expansion with depth limiting, paging, and pointer
//! dereference support.

use dap_types::types::Variable;

/// Configuration for variable expansion behavior.
#[derive(Debug, Clone)]
pub struct ExpansionConfig {
    /// Maximum recursion depth (default: 3).
    pub max_depth: usize,
    /// Maximum total variables to fetch across the entire expansion (prevents explosion).
    pub max_total_variables: usize,
    /// Whether to automatically dereference pointers via `evaluate("*name")`.
    pub dereference_pointers: bool,
    /// Max array elements to expand (prevents huge array expansions).
    pub max_array_elements: usize,
    /// Whether to attempt STL container expansion (requires evaluate calls).
    pub expand_stl_containers: bool,
    /// Max string length to display inline.
    pub max_string_length: usize,
}

impl Default for ExpansionConfig {
    fn default() -> Self {
        ExpansionConfig {
            max_depth: 3,
            max_total_variables: 500,
            dereference_pointers: true,
            max_array_elements: 50,
            expand_stl_containers: false,
            max_string_length: 256,
        }
    }
}

/// A variable that may have children (expanded or not).
#[derive(Debug, Clone)]
pub struct ExpandedVariable {
    /// The variable data from the adapter.
    pub variable: Variable,
    /// Child variables, if expanded. None = not yet expanded.
    pub children: Option<Vec<ExpandedVariable>>,
    /// Depth from the root of this expansion tree.
    pub depth: usize,
}

impl ExpandedVariable {
    /// Create an unexpanded variable node.
    pub fn new(variable: Variable, depth: usize) -> Self {
        ExpandedVariable {
            variable,
            children: None,
            depth,
        }
    }

    /// Returns whether this variable has expandable children.
    pub fn is_expandable(&self) -> bool {
        self.variable.variables_reference > 0
    }

    /// Returns the number of child variables.
    pub fn child_count(&self) -> usize {
        self.variable.named_variables.unwrap_or(0) as usize
            + self.variable.indexed_variables.unwrap_or(0) as usize
    }
}

/// Tracks expansion state to enforce depth and total-variable limits.
pub struct VariableExpander {
    config: ExpansionConfig,
    total_fetched: usize,
}

impl VariableExpander {
    /// Create a new expander with the given config.
    pub fn new(config: ExpansionConfig) -> Self {
        VariableExpander {
            config,
            total_fetched: 0,
        }
    }

    /// Returns the total number of variables fetched so far.
    pub fn total_fetched(&self) -> usize {
        self.total_fetched
    }

    /// Returns true if the total variable cap has been reached.
    pub fn is_capped(&self) -> bool {
        self.total_fetched >= self.config.max_total_variables
    }

    /// Reset the fetched counter (useful for repeated expansions).
    pub fn reset(&mut self) {
        self.total_fetched = 0;
    }

    /// Returns true if the given variable looks like a pointer type.
    pub fn is_pointer_type(var: &Variable) -> bool {
        var.var_type
            .as_ref()
            .map(|t| t.ends_with('*') || t.ends_with("*const") || t.ends_with("*mut"))
            .unwrap_or(false)
    }

    /// Returns true if the value looks like a memory address (hex pointer).
    pub fn looks_like_pointer_value(value: &str) -> bool {
        let trimmed = value.trim();
        trimmed.starts_with("0x")
            || (trimmed.len() >= 4
                && trimmed.chars().all(|c| c.is_ascii_hexdigit())
                && !trimmed.chars().all(|c| c == '0'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_var(name: &str, value: &str, var_type: Option<&str>, var_ref: u64) -> Variable {
        Variable {
            name: name.to_string(),
            value: value.to_string(),
            var_type: var_type.map(|s| s.to_string()),
            presentation_hint: None,
            evaluate_name: None,
            variables_reference: var_ref,
            named_variables: if var_ref > 0 { Some(2) } else { None },
            indexed_variables: None,
            memory_reference: None,
            declaration_location_reference: None,
            value_location_reference: None,
        }
    }

    #[test]
    fn test_expanded_variable_leaf() {
        let v = make_var("x", "42", Some("int"), 0);
        let ev = ExpandedVariable::new(v, 0);
        assert!(!ev.is_expandable());
        assert_eq!(ev.child_count(), 0);
        assert_eq!(ev.depth, 0);
    }

    #[test]
    fn test_expanded_variable_expandable() {
        let v = make_var("obj", "{...}", Some("MyStruct"), 1);
        let ev = ExpandedVariable::new(v, 1);
        assert!(ev.is_expandable());
        assert_eq!(ev.child_count(), 2); // named_variables = 2
    }

    #[test]
    fn test_is_pointer_type() {
        let var = make_var("p", "0x7fff", Some("int*"), 0);
        assert!(VariableExpander::is_pointer_type(&var));

        let var2 = make_var("n", "42", Some("int"), 0);
        assert!(!VariableExpander::is_pointer_type(&var2));

        let var3 = make_var("p", "0x7fff", None, 0);
        assert!(!VariableExpander::is_pointer_type(&var3));
    }

    #[test]
    fn test_looks_like_pointer_value() {
        assert!(VariableExpander::looks_like_pointer_value("0x7fff1234"));
        assert!(!VariableExpander::looks_like_pointer_value("42"));
        assert!(!VariableExpander::looks_like_pointer_value("hello"));
    }

    #[test]
    fn test_expansion_config_defaults() {
        let config = ExpansionConfig::default();
        assert_eq!(config.max_depth, 3);
        assert_eq!(config.max_total_variables, 500);
        assert!(config.dereference_pointers);
        assert_eq!(config.max_array_elements, 50);
        assert!(!config.expand_stl_containers);
        assert_eq!(config.max_string_length, 256);
    }

    #[test]
    fn test_expander_capped() {
        let config = ExpansionConfig {
            max_total_variables: 10,
            ..Default::default()
        };
        let mut expander = VariableExpander::new(config);
        assert!(!expander.is_capped());
        expander.total_fetched = 10;
        assert!(expander.is_capped());
    }

    #[test]
    fn test_expander_reset() {
        let config = ExpansionConfig::default();
        let mut expander = VariableExpander::new(config);
        expander.total_fetched = 100;
        expander.reset();
        assert_eq!(expander.total_fetched(), 0);
    }
}
