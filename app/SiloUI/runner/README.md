# silo-runner

Go helper that makes Silo the machine GitHub Actions jobs run on: a fresh Silo computer per job.
Setup, configuration and behaviour: [CI runner](../../../docs/SiloUI-CI-RUNNER.md).

```sh
go build -o silo-runner .
go vet ./... && go test -race ./...
```
