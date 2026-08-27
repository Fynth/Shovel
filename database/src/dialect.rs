use models::QueryFilterOperator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatFlavor {
    Postgres,
    Generic,
}

#[derive(Clone, Copy)]
pub struct Dialect {
    pub quote_identifier: fn(&str) -> String,
    pub filter_expression: fn(&str, QueryFilterOperator, &str) -> String,
    pub format_flavor: FormatFlavor,
}

pub fn quote_ident_double(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

pub fn quote_ident_backtick(identifier: &str) -> String {
    format!("`{}`", identifier.replace('`', "``"))
}
