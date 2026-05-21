FROM docker.io/library/ubuntu:24.04

# install tools and dependencies
RUN apt-get update && \
	DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
	ca-certificates && \
	# apt cleanup
	apt-get autoremove -y && \
	apt-get clean && \
	find /var/lib/apt/lists/ -type f -not -name lock -delete; \
	# add system user and link ~/.local/share/predictor-node to /data
	useradd --system --no-create-home --shell /usr/sbin/nologin -U polkadot && \
	mkdir -p /data /polkadot/.local/share && \
	chown -R polkadot:polkadot /data && \
	ln -s /data /polkadot/.local/share/predictor-node

USER polkadot

# copy the compiled binary to the container
COPY --chown=polkadot:polkadot --chmod=774 target/release/predictor-node /usr/bin/predictor-node

# check if executable works in this container
RUN /usr/bin/predictor-node --version

EXPOSE 30333 9933 9944 9615
VOLUME ["/data"]

ENTRYPOINT ["/usr/bin/predictor-node"]
