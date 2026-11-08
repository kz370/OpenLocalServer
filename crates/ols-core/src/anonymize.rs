//! Anonymizing rules for pulled server databases (§177).
//!
//! The rules live in `<project>/.openlocalserver/anonymize.yaml`. They hold no secrets, so a
//! team can commit them. Every table, column and key name is checked against
//! `^[A-Za-z_][A-Za-z0-9_]{0,63}$` and quoted per engine; `fixed:` text becomes a SQL string
//! literal through the service layer's escaping. Kinds are a closed set.

use std::collections::BTreeMap;

use serde_yaml_ng::Value;

/// The bcrypt hash of the word "password" that Laravel's factories use.
pub const PASSWORD_HASH: &str = "$2y$10$92IXUNpkjO0rOQ5byMi.Ye4oKoEa3Ro9llC/.og/at2.uheWG/igi";
const MAX_FIXED: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Email,
    Name,
    Phone,
    Password,
    Null,
    Empty,
    Fixed(String),
    Keep,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Table {
    Truncate,
    Columns {
        key: Option<String>,
        columns: BTreeMap<String, Kind>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rules {
    pub tables: BTreeMap<String, Table>,
}

pub fn valid_ident(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    name.len() <= 64
        && (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn ident(what: &str, name: &str) -> Result<(), String> {
    if valid_ident(name) {
        Ok(())
    } else {
        Err(format!(
            "{what} \"{name}\" is not allowed: use letters, digits and _ only, starting with a letter or _ (64 characters at most)."
        ))
    }
}

fn parse_kind(table: &str, column: &str, value: &Value) -> Result<Kind, String> {
    let text = match value {
        Value::Null => return Ok(Kind::Null),
        Value::String(s) => s.as_str(),
        _ => {
            return Err(format!(
                "{table}.{column}: the kind must be text such as email, name or \"fixed:Redacted\"."
            ))
        }
    };
    if let Some(fixed) = text.strip_prefix("fixed:") {
        if fixed.chars().count() > MAX_FIXED {
            return Err(format!(
                "{table}.{column}: fixed text can be {MAX_FIXED} characters at most."
            ));
        }
        return Ok(Kind::Fixed(fixed.to_string()));
    }
    Ok(match text {
        "email" => Kind::Email,
        "name" => Kind::Name,
        "phone" => Kind::Phone,
        "password" => Kind::Password,
        "null" => Kind::Null,
        "empty" => Kind::Empty,
        "keep" => Kind::Keep,
        other => {
            return Err(format!(
                "{table}.{column}: \"{other}\" is not a known kind. Use email, name, phone, password, null, empty, keep or fixed:<text>."
            ))
        }
    })
}

/// Reads and checks the rules file. Nothing is run here.
pub fn parse_rules(yaml: &str) -> Result<Rules, String> {
    let doc: Value = serde_yaml_ng::from_str(yaml)
        .map_err(|e| format!("The rules file is not valid YAML: {e}"))?;
    let map = doc
        .as_mapping()
        .ok_or("The rules file must be a mapping with `version` and `tables`.")?;
    match map.get("version") {
        None => {}
        Some(Value::Number(n)) if n.as_u64() == Some(1) => {}
        Some(_) => return Err("Only `version: 1` is supported.".into()),
    }
    let tables = match map.get("tables") {
        Some(Value::Mapping(t)) => t,
        Some(Value::Null) | None => return Ok(Rules::default()),
        Some(_) => return Err("`tables` must be a mapping of table name to rules.".into()),
    };
    let mut out = Rules::default();
    for (name, rule) in tables {
        let name = name.as_str().ok_or("A table name must be text.")?;
        ident("Table name", name)?;
        let table = match rule {
            Value::String(s) if s == "truncate" => Table::Truncate,
            Value::Mapping(m) => {
                let key = match m.get("key") {
                    None | Some(Value::Null) => None,
                    Some(Value::String(k)) => {
                        ident("Key column", k)?;
                        Some(k.clone())
                    }
                    Some(_) => return Err(format!("{name}: `key` must be a column name.")),
                };
                let mut columns = BTreeMap::new();
                match m.get("columns") {
                    Some(Value::Mapping(cols)) => {
                        for (c, kind) in cols {
                            let c = c.as_str().ok_or("A column name must be text.")?;
                            ident("Column name", c)?;
                            columns.insert(c.to_string(), parse_kind(name, c, kind)?);
                        }
                    }
                    None | Some(Value::Null) => {}
                    Some(_) => return Err(format!("{name}: `columns` must be a mapping.")),
                }
                if key.is_none()
                    && columns
                        .values()
                        .any(|k| matches!(k, Kind::Email | Kind::Name))
                {
                    return Err(format!(
                        "{name}: email and name need a `key` column (for example `key: id`) to make each value unique."
                    ));
                }
                Table::Columns { key, columns }
            }
            _ => {
                return Err(format!(
                    "{name}: write either `truncate` or a mapping with `key` and `columns`."
                ))
            }
        };
        out.tables.insert(name.to_string(), table);
    }
    Ok(out)
}

fn quote(engine: &str, name: &str) -> String {
    if engine == "postgres" {
        format!("\"{name}\"")
    } else {
        format!("`{name}`")
    }
}

fn expression(engine: &str, kind: &Kind, key: Option<&str>) -> Option<String> {
    let pg = engine == "postgres";
    let k = key.map(|k| quote(engine, k)).unwrap_or_default();
    Some(match kind {
        Kind::Email if pg => format!("'user_' || {k} || '@example.test'"),
        Kind::Email => format!("CONCAT('user_', {k}, '@example.test')"),
        Kind::Name if pg => format!("'User ' || {k}"),
        Kind::Name => format!("CONCAT('User ', {k})"),
        Kind::Phone => "'000-000-0000'".into(),
        Kind::Password => format!("'{PASSWORD_HASH}'"),
        Kind::Null => "NULL".into(),
        Kind::Empty => "''".into(),
        Kind::Fixed(text) if pg => crate::service::pg_string(text),
        Kind::Fixed(text) => crate::service::sql_string(text),
        Kind::Keep => return None,
    })
}

/// The statements for `engine` (`mariadb` or `postgres`), in table-name order.
pub fn build_sql(rules: &Rules, engine: &str) -> Result<Vec<String>, String> {
    if !matches!(engine, "mariadb" | "postgres") {
        return Err(format!(
            "Anonymizing supports MariaDB and PostgreSQL, not {engine}."
        ));
    }
    let mut out = Vec::new();
    for (table, rule) in &rules.tables {
        ident("Table name", table)?;
        match rule {
            Table::Truncate if engine == "postgres" => {
                out.push(format!("TRUNCATE TABLE {} CASCADE", quote(engine, table)))
            }
            Table::Truncate => out.push(format!("TRUNCATE TABLE {}", quote(engine, table))),
            Table::Columns { key, columns } => {
                let sets: Vec<String> = columns
                    .iter()
                    .filter_map(|(c, kind)| {
                        expression(engine, kind, key.as_deref())
                            .map(|e| format!("{} = {e}", quote(engine, c)))
                    })
                    .collect();
                if !sets.is_empty() {
                    out.push(format!(
                        "UPDATE {} SET {}",
                        quote(engine, table),
                        sets.join(", ")
                    ));
                }
            }
        }
    }
    Ok(out)
}

/// One script that runs all statements in a single session. MariaDB turns foreign key checks
/// off first so TRUNCATE works; PostgreSQL uses CASCADE.
pub fn script(statements: &[String], engine: &str) -> String {
    let mut text = String::new();
    if engine != "postgres" {
        text.push_str("SET FOREIGN_KEY_CHECKS=0; ");
    }
    for s in statements {
        text.push_str(s);
        text.push_str("; ");
    }
    if engine != "postgres" {
        text.push_str("SET FOREIGN_KEY_CHECKS=1;");
    }
    text
}

/// Fails, naming the first missing table or column, before anything is changed.
pub fn check_columns(rules: &Rules, existing: &[(String, String)]) -> Result<(), String> {
    for (table, rule) in &rules.tables {
        if !existing.iter().any(|(t, _)| t == table) {
            return Err(format!(
                "The rules mention the table \"{table}\", which the database does not have."
            ));
        }
        if let Table::Columns { key, columns } = rule {
            let wanted = columns.keys().chain(key.iter());
            for c in wanted {
                if !existing.iter().any(|(t, col)| t == table && col == c) {
                    return Err(format!(
                        "The rules mention the column \"{table}.{c}\", which the database does not have."
                    ));
                }
            }
        }
    }
    Ok(())
}

const TRUNCATE_TABLES: &[&str] = &[
    "sessions",
    "password_reset_tokens",
    "password_resets",
    "failed_jobs",
    "personal_access_tokens",
    "oauth_access_tokens",
];

fn suggest_kind(column: &str) -> Option<&'static str> {
    let c = column.to_ascii_lowercase();
    if c == "email" || c.ends_with("_email") {
        Some("email")
    } else if matches!(
        c.as_str(),
        "name" | "first_name" | "last_name" | "full_name" | "username"
    ) {
        Some("name")
    } else if matches!(c.as_str(), "phone" | "mobile") || c.ends_with("_phone") {
        Some("phone")
    } else if c == "password" {
        Some("password")
    } else if c.contains("token") || c.contains("secret") {
        Some("null")
    } else if c.starts_with("address")
        || matches!(
            c.as_str(),
            "street" | "city" | "zip" | "postcode" | "ip_address" | "last_login_ip"
        )
    {
        Some("fixed:Redacted")
    } else {
        None
    }
}

/// YAML text for the user to review, from `(table, column)` pairs of the local database.
pub fn suggest(columns: &[(String, String)]) -> String {
    let mut by_table: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (t, c) in columns {
        by_table.entry(t.as_str()).or_default().push(c.as_str());
    }
    let mut yaml = String::from("version: 1\ntables:\n");
    let mut any = false;
    for (table, cols) in &by_table {
        if !valid_ident(table) {
            continue;
        }
        if TRUNCATE_TABLES.contains(table) {
            yaml.push_str(&format!("  {table}: truncate\n"));
            any = true;
            continue;
        }
        let picks: Vec<(&str, &str)> = cols
            .iter()
            .filter(|c| valid_ident(c))
            .filter_map(|c| suggest_kind(c).map(|k| (*c, k)))
            .collect();
        if picks.is_empty() {
            continue;
        }
        let key = if cols.contains(&"id") { "id" } else { cols[0] };
        yaml.push_str(&format!("  {table}:\n    key: {key}\n    columns:\n"));
        for (c, k) in picks {
            yaml.push_str(&format!("      {c}: \"{k}\"\n"));
        }
        any = true;
    }
    if !any {
        yaml.push_str("  {}\n");
    }
    yaml
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
version: 1
tables:
  users:
    key: id
    columns:
      email: email
      name: name
      phone: phone
      password: password
      remember_token: "null"
      address: "fixed:Redacted"
  sessions: truncate
"#;

    #[test]
    fn mariadb_sql_for_the_example() {
        let rules = parse_rules(EXAMPLE).unwrap();
        let sql = build_sql(&rules, "mariadb").unwrap();
        assert_eq!(
            sql,
            vec![
                "TRUNCATE TABLE `sessions`".to_string(),
                format!("UPDATE `users` SET `address` = 'Redacted', `email` = CONCAT('user_', `id`, '@example.test'), `name` = CONCAT('User ', `id`), `password` = '{PASSWORD_HASH}', `phone` = '000-000-0000', `remember_token` = NULL"),
            ]
        );
        assert!(script(&sql, "mariadb").starts_with("SET FOREIGN_KEY_CHECKS=0; TRUNCATE"));
    }

    #[test]
    fn postgres_sql_for_the_example() {
        let rules = parse_rules(EXAMPLE).unwrap();
        let sql = build_sql(&rules, "postgres").unwrap();
        assert_eq!(
            sql,
            vec![
                "TRUNCATE TABLE \"sessions\" CASCADE".to_string(),
                format!("UPDATE \"users\" SET \"address\" = 'Redacted', \"email\" = 'user_' || \"id\" || '@example.test', \"name\" = 'User ' || \"id\", \"password\" = '{PASSWORD_HASH}', \"phone\" = '000-000-0000', \"remember_token\" = NULL"),
            ]
        );
    }

    #[test]
    fn bad_names_and_kinds_are_rejected() {
        for bad in [
            "tables:\n  \"users; drop\":\n    columns: {}\n",
            "tables:\n  users:\n    key: \"id`\"\n    columns:\n      a: phone\n",
            "tables:\n  users:\n    columns:\n      \"na me\": phone\n",
            "tables:\n  users:\n    key: id\n    columns:\n      a: shout\n",
            "tables:\n  users:\n    columns:\n      email: email\n",
            "version: 2\ntables: {}\n",
            "tables:\n  users: wipe\n",
        ] {
            assert!(parse_rules(bad).is_err(), "{bad}");
        }
        let long = format!(
            "tables:\n  u:\n    columns:\n      a: \"fixed:{}\"\n",
            "x".repeat(101)
        );
        assert!(parse_rules(&long).is_err());
    }

    #[test]
    fn fixed_text_is_escaped_for_each_engine() {
        let rules =
            parse_rules("tables:\n  t:\n    columns:\n      a: \"fixed:O'Neil\\\\x\"\n").unwrap();
        let my = build_sql(&rules, "mariadb").unwrap();
        assert_eq!(my[0], r"UPDATE `t` SET `a` = 'O''Neil\\x'");
        let pg = build_sql(&rules, "postgres").unwrap();
        assert_eq!(pg[0], r#"UPDATE "t" SET "a" = 'O''Neil\x'"#);
    }

    #[test]
    fn missing_tables_and_columns_are_named() {
        let rules = parse_rules(EXAMPLE).unwrap();
        let have = vec![
            ("users".to_string(), "id".to_string()),
            ("sessions".to_string(), "id".to_string()),
        ];
        let e = check_columns(&rules, &have).unwrap_err();
        assert!(
            e.contains("users.address") || e.contains("users.email"),
            "{e}"
        );
        let e = check_columns(&rules, &[]).unwrap_err();
        assert!(e.contains("\"sessions\""), "{e}");
    }

    #[test]
    fn suggestions_follow_the_naming_rules() {
        let cols: Vec<(String, String)> = [
            ("users", "id"),
            ("users", "email"),
            ("users", "first_name"),
            ("users", "remember_token"),
            ("users", "billing_phone"),
            ("users", "address_line"),
            ("users", "created_at"),
            ("sessions", "id"),
            ("posts", "title"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let yaml = suggest(&cols);
        assert!(yaml.contains("  sessions: truncate"));
        assert!(!yaml.contains("posts"));
        let rules = parse_rules(&yaml).unwrap();
        let Table::Columns { key, columns } = &rules.tables["users"] else {
            panic!("users");
        };
        assert_eq!(key.as_deref(), Some("id"));
        assert_eq!(columns["email"], Kind::Email);
        assert_eq!(columns["first_name"], Kind::Name);
        assert_eq!(columns["remember_token"], Kind::Null);
        assert_eq!(columns["billing_phone"], Kind::Phone);
        assert_eq!(columns["address_line"], Kind::Fixed("Redacted".into()));
        assert!(!columns.contains_key("created_at"));
    }
}
