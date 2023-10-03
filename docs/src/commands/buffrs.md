## buffrs

The official Buffrs command-line interface.

### Synopsis

`buffrs`

### Description

When invoked without any arguments, the Buffrs binary defaults to printing out
help information to the standard output.

This is equivalent to [`buffrs help`](buffrs-help.md).

### Output

```
Modern protobuf package management

Usage: buffrs <COMMAND>

Commands:
  init       Initializes a buffrs setup
  add        Adds dependencies to a manifest file
  remove     Removes dependencies from a manifest file
  package    Exports the current package into a distributable tgz archive
  publish    Packages and uploads this api to the registry
  install    Installs dependencies
  uninstall  Uninstalls dependencies
  generate   Generate code from installed buffrs packages
  login      Logs you in for a registry
  logout     Logs you out from a registry
  help       Print this message or the help of the given subcommand(s)

Options:
  -h, --help     Print help
  -V, --version  Print version
```