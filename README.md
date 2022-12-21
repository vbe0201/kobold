# Kobold

Kobold is a tool for deserializing, extracting and inspecting various file
formats used by KingsIsle in their games.

It supersedes [printrospector](https://gitlab.com/vale_/printrospector) and
extends its feature set.

## Supported formats

- **NAV:** NavigationGraph and ZoneNavigationGraph reading

- **BCD:** Binary Collision Data reading

- **POI:** Point of Interest data reading

- **WAD:** Archive introspection, validation, and extraction

- **ObjectProperty:** Serialization of binary state with additional
  `CoreObject` support

## Building the project

You will need an [installation of Rust](https://www.rust-lang.org/tools/install)
to build the project.

Then run the following commands to build and install kobold to your machine:

```shell
# Clone the repository
$ git clone https://github.com/vbe0201/kobold
$ cd kobold

# Install the CLI tool (can be invoked with kobold command)
$ cargo install --path cli

# OPTIONAL: Install the Python bindings.
# This assumes a recent installation of Python on the system.
$ cd py
$ python -m pip install .
```

## Library usage

There are currently no plans to publish `kobold` to crates.io, so for the
time being the preferred way to use it is:

```toml
# in Cargo.toml:

[dependencies]
kobold = { git = "https://github.com/vbe0201/kobold.git" }
```

## Using the CLI

For general help, see the output of the `--help` flag for `kobold` and its
individual subcommands.

### ObjectProperty types

For the `kobold op` subcommands to work properly, a type list must be provided.

These files are generated by an external project which requires an installation
of Python >= 3.10.

With an open Wizard101 game client, run:

```shell
$ pip install wiztype

# This requires that you are currently running the game client.
$ wiztype
```

The resulting file can then be passed to the `-t` option.

## Python API

TODO

## Licensing

The kobold library, the CLI tool, and the Python bindings are collectively
licensed under the terms of the [ISC License](./LICENSE).
