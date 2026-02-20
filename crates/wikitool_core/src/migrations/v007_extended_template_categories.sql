-- v007: add template category mappings that were previously hardcoded in Rust
-- but missing from the v004 seed data.

INSERT OR IGNORE INTO template_category_mappings (prefix, category) VALUES
    ('Template:Translation', 'translations'),
    ('Module:Translation', 'translations'),
    ('Template:Birth date', 'date'),
    ('Template:Start date', 'date'),
    ('Template:End date', 'date'),
    ('Module:Age', 'date'),
    ('Template:Remilia navigation', 'navigation');
