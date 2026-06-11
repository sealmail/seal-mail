use crate::models::*;

pub fn rule_matches(rule: &FilterRule, mail: &EmailFull) -> bool {
    if !rule.enabled {
        return false;
    }
    if let Some(acc) = &rule.account_id {
        if acc != &mail.meta.account_id {
            return false;
        }
    }
    let haystack = match rule.field.as_str() {
        "from" => format!("{} {}", mail.meta.from_name, mail.meta.from_addr),
        "to" => mail.to.join(" "),
        "subject" => mail.meta.subject.clone(),
        "body" => mail.body_text.clone(),
        _ => return false,
    }
    .to_lowercase();
    let needle = rule.value.to_lowercase();
    match rule.op.as_str() {
        "contains" => haystack.contains(&needle),
        "not_contains" => !haystack.contains(&needle),
        "equals" => haystack.trim() == needle.trim(),
        "starts_with" => haystack.starts_with(&needle),
        "ends_with" => haystack.ends_with(&needle),
        _ => false,
    }
}
