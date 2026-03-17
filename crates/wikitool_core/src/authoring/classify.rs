use crate::knowledge::model::AuthoringKnowledgePackResult;

use super::model::ArticleType;

pub(crate) fn classify_article_type(pack: &AuthoringKnowledgePackResult) -> ArticleType {
    let lower_topic = pack.topic.to_ascii_lowercase();
    let lower_templates = pack
        .suggested_templates
        .iter()
        .map(|template| template.template_title.to_ascii_lowercase())
        .collect::<Vec<_>>();

    if lower_templates
        .iter()
        .any(|title| title.contains("infobox person"))
    {
        return ArticleType::Person;
    }
    if lower_templates
        .iter()
        .any(|title| title.contains("infobox organization"))
    {
        return ArticleType::Organization;
    }
    if lower_templates
        .iter()
        .any(|title| title.contains("infobox website"))
    {
        return ArticleType::Website;
    }
    if lower_templates
        .iter()
        .any(|title| title.contains("infobox event"))
    {
        return ArticleType::Event;
    }
    if lower_templates
        .iter()
        .any(|title| title.contains("infobox artwork") || title.contains("infobox exhibition"))
    {
        return ArticleType::Work;
    }
    if lower_templates
        .iter()
        .any(|title| title.contains("infobox nft collection") || title.contains("collection"))
    {
        return ArticleType::Collection;
    }
    if lower_topic.contains("collective")
        || lower_topic.contains("organization")
        || lower_topic.contains("corporation")
    {
        return ArticleType::Organization;
    }
    if lower_topic.contains("website")
        || lower_topic.contains(".org")
        || lower_topic.contains(".com")
    {
        return ArticleType::Website;
    }
    ArticleType::Unknown
}
