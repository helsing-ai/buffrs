# Contributing

Pull requests are welcome. For major changes, please open an issue first
to discuss what you would like to change.

Please make sure to update tests as appropriate.

You can set-up a pre-commit hooks that automatically runs `cargo fmt` and `cargo clippy` by running:

```bash
ln -s ../../pre-commit .git/hook
```

**Note:** running Clippy can take a while the first while, but subsequent run
should only take a second or so.
