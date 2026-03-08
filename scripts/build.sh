#!/bin/bash

# Fluxbase Build Script
# Usage: ./scripts/build.sh [--docker] [--service <name>]

set -e

# Default values
DOCKER_BUILD=false
SERVICE_NAME="all"

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --docker) DOCKER_BUILD=true ;;
        --service) SERVICE_NAME="$2"; shift ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

SERVICES=("api" "gateway" "runtime" "queue" "cli")

build_rust_service() {
    local service=$1
    echo "Building Rust service: $service..."
    if [ "$DOCKER_BUILD" = true ]; then
        if [ -f "$service/Dockerfile" ]; then
            echo "Building Docker image for $service..."
            docker build -t "fluxbase-$service:latest" -f "$service/Dockerfile" .
        else
            echo "Warning: No Dockerfile found for $service, skipping Docker build."
            cargo build --release -p "$service"
        fi
    else
        SQLX_OFFLINE=true cargo build --release -p "$service"
    fi
}

build_dashboard() {
    echo "Building Dashboard..."
    cd dashboard
    npm install
    npm run build
    cd ..
}

if [ "$SERVICE_NAME" == "all" ]; then
    for service in "${SERVICES[@]}"; do
        build_rust_service "$service"
    done
    build_dashboard
else
    if [[ " ${SERVICES[@]} " =~ " ${SERVICE_NAME} " ]]; then
        build_rust_service "$SERVICE_NAME"
    elif [ "$SERVICE_NAME" == "dashboard" ]; then
        build_dashboard
    else
        echo "Error: Unknown service $SERVICE_NAME"
        exit 1
    fi
fi

echo "Build complete!"
