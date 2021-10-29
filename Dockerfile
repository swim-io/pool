FROM rust:latest

WORKDIR app

RUN cargo install honggfuzz
RUN sh -c "$(curl -sSfL https://release.solana.com/v1.7.6/install)"
RUN apt-get update
RUN apt-get install -y build-essential binutils-dev libunwind-dev libblocksruntime-dev liblzma-dev lldb rust-lldb

COPY . .

# don't include [] or the command will be run directly rather than inside a shell - https://goinbigdata.com/docker-run-vs-cmd-vs-entrypoint/
CMD cd fuzz; cargo hfuzz run pool_fuzz