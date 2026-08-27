# Wikitest

Wikitest is Wikitool's executable dogfooding laboratory. Scenario and suite manifests in this
directory exercise the public `wikitool` binary; run artifacts and hash-bound receipts are written
under `.wikitest/`.

Deterministic scenarios may establish process, structured-output, filesystem, hash, and
completeness facts. They do not establish prose quality. Editorial authoring and adjudication use a
separate protocol so that a passing lint or a fluent draft can never masquerade as reader-value
evidence.
