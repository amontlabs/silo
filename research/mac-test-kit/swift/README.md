# silovm: the kit's VM helper

A small command line program on Apple's Virtualization framework. `./kit build` compiles it with
`swiftc` and signs it ad hoc with the `com.apple.security.virtualization` entitlement, into
`~/SiloKit/bin/silovm`. Every VM test goes through it, so the tests measure the framework the way
Silo would use it, with no Tart or UTM in between.

It never touches anything outside the bundle folders it is given, except reading
`/var/db/dhcpd_leases` to find a computer's address on Apple's NAT.

## A bundle

A folder ending in `.silovm`:

| File | Holds |
|---|---|
| `disk.img` | The main disk, raw or ASIF |
| `aux.img` | The auxiliary storage (`VZMacAuxiliaryStorage`) |
| `hardware-model.bin` | `VZMacHardwareModel.dataRepresentation` |
| `machine-id.bin` | `VZMacMachineIdentifier.dataRepresentation` |
| `config.json` | `{"mac": "..", "cpus": 4, "memory_gb": 8, "disk_format": "raw"}` |
| `ctl.sock` | The control socket while `silovm run` is running |

## Commands

```
silovm version
silovm host-check
    One JSON object: macos, build, chip, virtualization_supported, provisioning_api (bool).
silovm ipsw-url
    One JSON object: url, build, os_version of VZMacOSRestoreImage.latestSupported.
silovm install --ipsw PATH --out BUNDLE [--disk-gb 80] [--cpus 4] [--memory-gb 8]
               [--disk-format raw|asif]
    Makes a new bundle and installs macOS into it with VZMacOSInstaller. Progress lines go to
    stderr. The computer is never started: the bundle is "the installed system" of
    computer-image.
silovm clone --from BUNDLE --to BUNDLE [--new-identity]
    clonefile(2) of every file. --new-identity writes a new machine identifier, new auxiliary
    storage from the same hardware model, and a new MAC address.
silovm run --bundle BUNDLE [options]
    Starts the computer and stays in the foreground until it stops. Prints one JSON object per
    line on stdout (below). SIGTERM asks the guest to stop, SIGKILL is not caught.
      --provision-user NAME --provision-password PW [--provision-full-name TEXT]
      [--no-autologin] [--no-ssh]        VZMacGuestProvisioningOptions, first start only
      --net nat|none|vfkit:SOCKETPATH     vfkit: a gvproxy listening with -listen-vfkit unixgram://
                                          SOCKETPATH. silovm binds its own datagram socket at
                                          BUNDLE/net.sock, connects it to SOCKETPATH and sends one
                                          "VFKT" datagram first, so gvproxy learns its peer (as vfkit
                                          does). Both paths must be under 104 bytes.
      --mtu N                             with --net vfkit only: the attachment's
                                          maximumTransmissionUnit, 1500 (default) to 65535
      --gui                               a window with the screen (to see Setup Assistant)
      --disk-cache automatic|cached|uncached
      --disk-sync full|fsync|none         VZDiskImageStorageDeviceAttachment's two modes
      --extra-disk PATH[:ro]              a second disk, repeatable
      --max-seconds N                     force a stop after N seconds
      --continue-on-provisioning-error    start anyway when provisioning is refused
silovm ctl --bundle BUNDLE COMMAND
    Talks to a running `silovm run` through ctl.sock. Prints one JSON object.
      state | request-stop | stop | pause | resume | screenshot PATH (only with --gui)
silovm ip --bundle BUNDLE [--wait SECONDS]
    The computer's address on Apple's NAT, from /var/db/dhcpd_leases by the bundle's MAC.
```

## Events of `silovm run`

```
{"event":"config","mac":"..","cpus":4,"memory_gb":8,"net":"nat","mtu":null,"disk_cache":"automatic","disk_sync":"full",...}
{"event":"provisioning","stage":"validate"|"set"|"start","ok":true}
{"event":"provisioning","stage":"validate"|"set"|"start","ok":false,"domain":"VZErrorDomain","code":40002,"name":"guestProvisioningInvalidUsername","message":".."}
{"event":"started","t":1764000000.123}
{"event":"state","value":"running"|"paused"|"stopping"|"error"}
{"event":"stopped","how":"guest"|"requested"|"forced"|"max-seconds"|"error","error":"..","t":..}
```

Every error from the framework is printed with its domain, code and the `VZError.Code` name when
there is one. Nothing is swallowed with `try?`: a refused provisioning is an event, and the run
stops unless `--continue-on-provisioning-error` is given (test 01 uses it to see whether the
computer then shows Setup Assistant).
