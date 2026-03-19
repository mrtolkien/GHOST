# Tailscale Remote Access

Tailscale gives the OPERATOR secure remote access to GHOST and its services from any
device — phone, laptop, another machine — without opening any ports on the home router
or server.

## What Tailscale Provides

- **Secure mesh VPN** — WireGuard-based, end-to-end encrypted connections between all
  the OPERATOR's devices. No port forwarding required.
- **No open ports** — GHOST's services (web UI, APIs) are never exposed to the public
  internet. Only authenticated Tailscale devices can reach them.
- **MagicDNS** — each device gets a stable `<device>.tail<hash>.ts.net` hostname, so
  the OPERATOR can reach services by name from anywhere.
- **Access controls** — fine-grained ACLs in the Tailscale admin panel control which
  devices can talk to which services.

## Installation

Follow the official instructions for the OPERATOR's platform:
https://tailscale.com/download

On Linux (Debian/Ubuntu):

```
curl -fsSL https://tailscale.com/install.sh | sh
```

On macOS: install via the App Store or the `.pkg` from the download page.

On NixOS: add `services.tailscale.enable = true;` to the system configuration.

## Initial Setup

Authenticate and connect to the OPERATOR's tailnet:

```
sudo tailscale up
```

This opens a browser window for authentication. After login, the machine appears in the
Tailscale admin panel at https://login.tailscale.com/admin/machines.

Verify the connection:

```
tailscale status
tailscale ip -4      # show this machine's Tailscale IP
```

## Exposing GHOST Services

Use `tailscale serve` to proxy specific local ports over Tailscale, making them
accessible from other devices in the tailnet without exposing them to the internet.

Expose the GHOST web interface (if running) on Tailscale port 443:

```
sudo tailscale serve https / http://127.0.0.1:<ghost-port>
```

Expose the SigNoz UI:

```
sudo tailscale serve https:3301 / http://127.0.0.1:3301
```

List currently served endpoints:

```
tailscale serve status
```

Remove a served endpoint:

```
sudo tailscale serve https:3301 off
```

## Funnel (Public Internet Exposure)

`tailscale funnel` makes a service reachable from the public internet (not just the
tailnet). Use this only when the OPERATOR explicitly needs a public endpoint — for
example, a webhook receiver.

```
sudo tailscale funnel 443 on
```

This is separate from `serve` and requires enabling Funnel in the Tailscale admin panel
under **DNS > HTTPS Certificates** and **ACLs > Funnel**.

## ACL Considerations

By default, all devices in the tailnet can reach each other. The OPERATOR may want to
restrict access — for example, only allowing their phone to reach GHOST services.

Edit the ACL policy at https://login.tailscale.com/admin/acls. A minimal policy that
allows only tagged devices:

```json
{
  "acls": [
    {
      "action": "accept",
      "src": ["tag:personal-devices"],
      "dst": ["tag:ghost-server:*"]
    }
  ],
  "tagOwners": {
    "tag:personal-devices": ["autogroup:owner"],
    "tag:ghost-server": ["autogroup:owner"]
  }
}
```

Tag the GHOST machine in the admin panel, then apply the tag during `tailscale up`:

```
sudo tailscale up --advertise-tags=tag:ghost-server
```

## Checking Connection Health

```
tailscale ping <other-device>    # round-trip latency to another tailnet device
tailscale netcheck               # diagnose connectivity issues
```

If a device shows as offline in the admin panel:

```
sudo tailscale up --reset        # re-authenticate if needed
sudo systemctl restart tailscaled
```
