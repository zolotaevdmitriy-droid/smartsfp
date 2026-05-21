# systemd units for ACM-UZ on ISM4120I

Drop these into `/etc/systemd/system/` on the module (typically delivered
through the OTA `update.tar.gz`). After install:

```bash
systemctl daemon-reload
systemctl enable --now acm-cryptod
systemctl enable --now acm-agent
```

## Files

| Unit | Purpose | CPU | RAM cap |
|---|---|---|---|
| `acm-cryptod.service` | Rust DPDK datapath + MUSDK SAM AES | core 1 | 650 MiB |
| `acm-agent.service` | Go management agent + Svelte web UI | core 0 | 256 MiB |

## Dependencies on the module

Required to be in place before the units start:

- Kernel modules `musdk_cma` and `rte_kni` loaded at boot
  (`/etc/modules-load.d/modules.conf` — vendor's stock setup already
  includes these).
- Hugepages reserved at boot via kernel cmdline `hugepages=200`
  (vendor's stock setup).
- User `acm` and group `acm` created (post-install script of the OTA package).
- Directories `/run/acm/`, `/var/lib/acm/`, `/etc/acm/` created with
  proper owners (post-install).
- TLS materials in `/etc/acm/ca/` for mTLS with the controller.

## What's NOT here yet

- `acm-update.timer` + `acm-update.service` — periodic check for OTA
  updates from the controller. Will be added when OTA flow is fleshed out.
- `acm-cryptod-watchdog.service` — separate watchdog for sudden death
  detection, if WATCHDOG_USEC turns out insufficient.
