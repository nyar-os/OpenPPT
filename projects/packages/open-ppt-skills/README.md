# @wae/open-ppt-skills

Agent skills for OpenPPT. The package ships skill documents only; it is not a
runtime library and contains no XML, ZIP, typesetting or drawing algorithms.

## Status

Scaffold only. `skills/open-ppt/SKILL.md` records the routing and evidence rules
for create, edit, template fill, review and delivery, but no SDK or CLI
capability exists yet, so none of those routes is executable.

## Layout

```text
packages/open-ppt-skills/
├── package.json
├── README.md
└── skills/
    └── open-ppt/
        └── SKILL.md
```

Design rules, the format/support matrix and executable examples are added with
the first implemented release. The skill must never be shipped claiming methods,
commands or flags that the installed SDK/CLI does not expose.
