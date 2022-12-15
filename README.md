# D-Prom

Protocol for exposing Prometheus metrics via D-Bus. This repo contains D-Bus interface definitions under [`dbus/`](/dbus) as well as the programs `dprom-export` and `dprom-file-gauge`.

`dprom-export` is a Prometheus-compatible metrics exporter supporting optional TLS with mutual authentication and live monitoring of D-Prom interfaces on the D-Bus.

`dprom-file-gauge` is a reference implementation of a service exposing the D-Prom interfaces.
This command periodically refreshes the configured files and exports the float values of these files to D-Bus via the D-Prom interfaces.

Example configuration files for both of these services can be found in the [`config_examples/`](/config_examples) directory.

## Installing

### Arch Linux

Pre-built packages are available on the [releases](https://github.com/haileys/dprom/releases) page.

Packages can also be built from source using the `PKGBUILD` in the [`pkg/arch/`](/pkg/arch) directory:

```sh-session
$ cd pkg/arch
$ makepkg -si
... will build package and prompt to install with sudo
```
