---
"silo-ui": patch
---

Silo now lives at github.com/amontlabs/silo and silo.amontlabs.com, and Debian installs get updates from apt.silo.amontlabs.com. Existing Debian installs must run `sudo sed -i 's#https://0xpolarzero.github.io/silo/apt#https://apt.silo.amontlabs.com/apt#' /etc/apt/sources.list.d/silo.sources` once to keep receiving updates.

The Silo GitHub App is now `silo-amont-labs`, owned by Amont Labs. Update Silo to keep using GitHub repository access; earlier versions look for the app under its previous name.
