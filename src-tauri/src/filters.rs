use crate::models::*;

/// 一组待批量移动的邮件：同目标目录 + 同标已读策略合并，单条 IMAP 命令完成
pub struct MovePlan {
    pub target: String,
    pub mark_read: bool,
    pub uids: Vec<u32>,
    /// 与 uids 一一对应的主题，用于整理明细展示
    pub subjects: Vec<String>,
}

/// 按规则给邮件生成移动计划：每封邮件取第一条命中的规则（规则按顺序优先），
/// 相同 (目标目录, 标已读) 合并为一组；目标等于来源目录的跳过
pub fn plan_moves(
    rules: &[FilterRule],
    account_id: &str,
    source_folder: &str,
    mails: &[EmailFull],
) -> Vec<MovePlan> {
    let mut plans: Vec<MovePlan> = Vec::new();
    for mail in mails {
        let Some(rule) = rules.iter().find(|rule| {
            rule.enabled
                && rule
                    .account_id
                    .as_ref()
                    .map(|id| id == account_id)
                    .unwrap_or(true)
                && rule.target_folder != source_folder
                && rule_matches(rule, mail)
        }) else {
            continue;
        };
        match plans
            .iter_mut()
            .find(|p| p.target == rule.target_folder && p.mark_read == rule.mark_read)
        {
            Some(plan) => {
                plan.uids.push(mail.meta.uid);
                plan.subjects.push(mail.meta.subject.clone());
            }
            None => plans.push(MovePlan {
                target: rule.target_folder.clone(),
                mark_read: rule.mark_read,
                uids: vec![mail.meta.uid],
                subjects: vec![mail.meta.subject.clone()],
            }),
        }
    }
    plans
}

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

/// 邮件是否会被过滤规则移出 `source_folder`（如屏蔽发件人 → 垃圾邮件）。
/// 用于新邮件通知：命中的邮件仍触发前端同步，但**不弹系统通知**。
pub fn would_move_out(
    rules: &[FilterRule],
    account_id: &str,
    source_folder: &str,
    mail: &EmailFull,
) -> bool {
    rules.iter().any(|rule| {
        rule.enabled
            && rule
                .account_id
                .as_ref()
                .map(|id| id == account_id)
                .unwrap_or(true)
            && rule.target_folder != source_folder
            && rule_matches(rule, mail)
    })
}
