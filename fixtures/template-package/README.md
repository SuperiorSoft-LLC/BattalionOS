Made by `templates/page-app/scripts/package.mjs` from `fixtures/page-app`, signed with a THROWAWAY key (no real key):

    node templates/page-app/scripts/package.mjs --app fixtures/page-app --out fixtures/template-package --base-url https://example.invalid/page-app/

`crates/battalion-host/tests/template_package.rs` installs it through the Armory (urls rewritten to `file:`), so the
template's packer is checked by the OS's own package check. Regenerate it with the command above when the packer changes.
