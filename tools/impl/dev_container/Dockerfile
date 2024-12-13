FROM rust

RUN rustup target add aarch64-linux-android

RUN apt-get update && apt-get install --yes --no-install-recommends \
		net-tools \
	# Clear apt cache to save space in layer.
	&& rm -rf /var/lib/apt/lists/*

RUN wget https://dl.google.com/android/repository/platform-tools-latest-linux.zip && \
	unzip -d /usr/local platform-tools-latest-linux.zip

ENV PATH="/usr/local/platform-tools:${PATH}"
