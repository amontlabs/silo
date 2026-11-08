# silo-runner

Go helper that makes Silo the machine GitHub Actions jobs run on: a fresh Silo computer per job.
Setup, configuration and behaviour: [CI runner](../../../docs/SiloUI-CI-RUNNER.md). On a Linux host it ships in
the engine archive and runs as a systemd user service ([Linux hosts](../../../docs/SiloUI-CI-RUNNER.md#linux-hosts)).

```sh
go build -o silo-runner .
go vet ./... && go test -race ./...
```
