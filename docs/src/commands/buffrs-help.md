## buffrs help

Prints out information about how to use the CLI.

### Synopsis

`buffrs help [command]`

### Description

When called by itself, this command lists all the supported commands along with
a brief description.

When called with a command argument, it will provide specific help for that
command.

Passing the `-h` or `--help` flags is equivalent to invoking this command.

### Examples

```
> buffrs help
Modern protobuf package management

Usage: buffrs <COMMAND>

Commands:
  init       Initializes a buffrs setup
  lint       Check rule violations for this package
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

```
> buffrs help init
Initializes a buffrs setup

Usage: buffrs init [OPTIONS] [PACKAGE]

Arguments:
  [PACKAGE]  The package name used for initialization

Options:
      --lib      Sets up the package as lib
      --api      Sets up the package as api
  -h, --help     Print help
  -V, --version  Print version
```
