# Import this module from configuration.nix.
# Install the correct static binary as /opt/homelab/homelab (root-owned, mode 0755).
# After nixos-rebuild switch, initialize before starting the service:
# sudo -u homelab /opt/homelab/homelab --data-dir /var/lib/homelab init
# sudo systemctl start homelab
{ ... }:
{
  users.groups.homelab = {};
  users.users.homelab = {
    isSystemUser = true;
    group = "homelab";
    home = "/var/lib/homelab";
  };
  systemd.tmpfiles.rules = [ "d /var/lib/homelab 0700 homelab homelab -" ];
  systemd.services.homelab = {
    description = "Home Lab Manager";
    wantedBy = [ "multi-user.target" ];
    after = [ "network.target" ];
    unitConfig.ConditionPathExists = "/var/lib/homelab/homelab.redb";
    serviceConfig = {
      User = "homelab";
      Group = "homelab";
      StateDirectory = "homelab";
      StateDirectoryMode = "0700";
      UMask = "0077";
      ExecStart = "/opt/homelab/homelab --data-dir /var/lib/homelab serve --listen 0.0.0.0:8080";
      Restart = "on-failure";
      RestartSec = 5;
      NoNewPrivileges = true;
      PrivateTmp = true;
      ProtectSystem = "strict";
      ProtectHome = true;
      ReadWritePaths = [ "/var/lib/homelab" ];
    };
  };
  # Open TCP 8080 only on a trusted LAN/VPN interface, for example:
  # networking.firewall.interfaces."wg0".allowedTCPPorts = [ 8080 ];
}
