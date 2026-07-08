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
    // from/to 拆成多个候选串（显示名、地址），任一命中即匹配；
    // 否则「等于 某地址」会因为显示名和地址拼接而永远匹配不上
    let haystacks: Vec<String> = match rule.field.as_str() {
        "from" => vec![mail.meta.from_name.clone(), mail.meta.from_addr.clone()],
        "to" => mail.to.clone(),
        "subject" => vec![mail.meta.subject.clone()],
        "body" => vec![mail.body_text.clone()],
        _ => return false,
    }
    .into_iter()
    .map(|h| h.to_lowercase())
    .collect();
    let needle = rule.value.to_lowercase();
    match rule.op.as_str() {
        "contains" => haystacks.iter().any(|h| h.contains(&needle)),
        "not_contains" => !haystacks.iter().any(|h| h.contains(&needle)),
        "equals" => haystacks.iter().any(|h| h.trim() == needle.trim()),
        "starts_with" => haystacks.iter().any(|h| h.starts_with(&needle)),
        "ends_with" => haystacks.iter().any(|h| h.ends_with(&needle)),
        _ => false,
    }
}
