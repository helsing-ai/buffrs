## buffrs

The official Buffrs command-line interface.

### Synopsis

`buffrs`

### Description

When invoked without any arguments, the Buffrs binary defaults to printing out
help information to the standard output.

This is equivalent to [`buffrs help`](buffrs-help.md), or invoking with the `-h`
or `--help` flags.

Providing the `-V` or `--version` flags prints the current version of buffrs.

### Output

```
Modern protobuf package management

Usage: buffrs [OPTIONS] <COMMAND>

Commands:
  init       Initializes a buffrs setup
  new        Creates a new buffrs package in the current directory
  lint       Check rule violations for this package
  add        Adds dependencies to a manifest file
  remove     Removes dependencies from a manifest file
  package    Exports the current package into a distributable tgz archive
  publish    Packages and uploads this api to the registry
  install    Installs dependencies
  uninstall  Uninstalls dependencies
  list       Lists all protobuf files managed by Buffrs to stdout
  login      Logs you in for a registry
  logout     Logs you out from a registry
  lock       Lockfile related commands
  help       Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose  Enable verbose logging [env: BUFFRS_VERBOSE=]
  -h, --help     Print help
  -V, --version  Print version
```
