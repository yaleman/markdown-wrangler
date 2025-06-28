# Build stage
FROM rustlang/rust:nightly-slim AS builder

# Set working directory
WORKDIR /app

# Copy dependency files first for better caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (this layer will be cached)
RUN cargo build --release

# Remove dummy files
RUN rm -rf src

# Copy source code
COPY src ./src

# Build the actual application
RUN cargo build --release

# Runtime stage
FROM gcr.io/distroless/cc-debian12

# Set working directory
WORKDIR /app

# Copy the binary from builder stage
COPY --from=builder /app/target/release/markdown-wrangler /app/markdown-wrangler

# Copy static assets
COPY static ./static

# Expose port
EXPOSE 5420

# Set the binary as entrypoint
ENTRYPOINT ["/app/markdown-wrangler"]