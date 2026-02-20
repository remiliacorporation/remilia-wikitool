-- v006: template invocation metadata for drift-resistant authoring
-- Captures real template parameter signatures seen in local pages.

CREATE TABLE IF NOT EXISTS indexed_template_invocations (
    source_relative_path TEXT NOT NULL,
    source_title TEXT NOT NULL,
    template_title TEXT NOT NULL,
    parameter_keys TEXT NOT NULL,
    PRIMARY KEY (source_relative_path, template_title, parameter_keys),
    FOREIGN KEY (source_relative_path) REFERENCES indexed_pages(relative_path) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_indexed_template_invocations_template
    ON indexed_template_invocations(template_title);
CREATE INDEX IF NOT EXISTS idx_indexed_template_invocations_source
    ON indexed_template_invocations(source_title);
