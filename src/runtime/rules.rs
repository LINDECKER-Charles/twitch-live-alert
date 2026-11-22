use super::{Control, ControlError, SETTINGS_ERROR, category_error};
use crate::rules::{MAX_RULES, Rule};

impl Control {
    pub(crate) fn rules(&self) -> Vec<Rule> {
        self.settings.borrow().rules()
    }

    pub(crate) fn display_rules(&self) -> Vec<Rule> {
        let mut rules = self.rules();
        let session = self.twitch.borrow().clone();
        if let Some(session) = session {
            let cached = session.cached_categories();
            for rule in &mut rules {
                if rule.box_art_url.is_empty()
                    && let Some(category) = cached.get(&rule.filter.category.to_lowercase())
                {
                    rule.box_art_url = category.box_art_url.clone();
                }
            }
        }
        rules
    }

    pub(crate) async fn upsert_rule(&self, mut rule: Rule) -> Result<Vec<Rule>, ControlError> {
        let _guard = self.changes.lock().await;
        let mut settings = self.settings.borrow().clone();
        let mut rules = settings.rules();
        let existing = rules.iter().position(|r| r.id == rule.id);
        if !rule.id.is_empty() && existing.is_none() {
            return Err(ControlError::Invalid("Cette règle n’existe plus."));
        }
        if existing.is_none() && rules.len() >= MAX_RULES {
            return Err(ControlError::Invalid(
                "Le maximum de 64 règles est atteint.",
            ));
        }
        rule.name = rule.name.trim().to_owned();
        rule.filter.category = rule.filter.category.trim().to_owned();
        rule.filter.language = rule.filter.language.trim().to_ascii_lowercase();
        if !rule.validate() {
            return Err(ControlError::Invalid(
                "Vérifiez le nom, la catégorie, la langue, les canaux et les horaires de la règle.",
            ));
        }
        if let Some(previous) = existing
            .map(|i| &rules[i])
            .filter(|r| r.filter.category == rule.filter.category)
        {
            rule.box_art_url = previous.box_art_url.clone();
        } else {
            let mut twitch = self
                .twitch
                .borrow()
                .clone()
                .ok_or(ControlError::Unavailable(
                    "Connectez Twitch pour valider la catégorie.",
                ))?;
            let category = twitch
                .category(&rule.filter.category)
                .await
                .map_err(category_error)?;
            rule.filter.category = category.name;
            rule.box_art_url = category.box_art_url;
        }
        if let Some(i) = existing {
            rules[i] = rule;
        } else {
            rule.id = uuid::Uuid::new_v4().to_string();
            rules.push(rule);
        }
        settings.rules = Some(rules.clone());
        settings
            .save(&self.settings_path)
            .map_err(|_| ControlError::Operation(SETTINGS_ERROR))?;
        self.apply(settings);
        Ok(rules)
    }

    pub(crate) async fn delete_rule(&self, id: &str) -> Result<Vec<Rule>, ControlError> {
        let _guard = self.changes.lock().await;
        let mut settings = self.settings.borrow().clone();
        let mut rules = settings.rules();
        let index = rules
            .iter()
            .position(|rule| rule.id == id)
            .ok_or(ControlError::Invalid("Cette règle n’existe plus."))?;
        rules.remove(index);
        settings.rules = Some(rules.clone());
        settings
            .save(&self.settings_path)
            .map_err(|_| ControlError::Operation(SETTINGS_ERROR))?;
        self.apply(settings);
        Ok(rules)
    }
}
