# Package Name Specifications

Buffrs package names must conform to the following rules:

- Must be **at least 1** character long.
- Must be **at most 128** characters long.
- Must **start with** an ASCII lowercase alphabetic character (`a`–`z`).
- Must consist **only of** ASCII lowercase letters (`a`–`z`) and hyphens (`-`).

In other words, valid package names match the regular expression
`^[a-z][a-z-]{0,127}$`.

## Examples

Valid names:
- `physics`
- `common-types`
- `my-api`

Invalid names:
- `Physics` – uppercase letters not allowed
- `my_api` – underscores not allowed
- `1my-api` – must start with a letter
- (empty string) – must be at least one character

## Relationship to Protocol Buffer Package Names

The Buffrs package name is used as the required prefix for all Protocol Buffer
`package` declarations inside the package. For example, a Buffrs package named
`physics` must declare protocol buffer packages matching `physics` or
`physics.*`.

See [Protocol Buffer Rules](protocol-buffer-rules.md) for more information.
