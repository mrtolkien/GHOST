FROM nixos/nix:latest

RUN echo "experimental-features = nix-command flakes" >> /etc/nix/nix.conf

COPY flake.nix flake.lock /opt/ghost-flake/

COPY deploy/common/entrypoint.sh /opt/ghost/entrypoint.sh
RUN chmod +x /opt/ghost/entrypoint.sh

ENV GHOST_CONFIG_DIR=/config
ENV GHOST_WORKSPACE=/workspace

ENTRYPOINT ["/opt/ghost/entrypoint.sh"]
