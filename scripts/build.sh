#!/bin/bash

# Fluxbase Build Script
# Usage: ./scripts/build.sh [--docker] [--service <name>]

set -e

# Default values
DOCKER_BUILD=false
SERVICE_NAME="all"
REGISTRY=""
PLATFORM=""
PARALLEL=false

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --docker) DOCKER_BUILD=true ;;
        --service) SERVICE_NAME="$2"; shift ;;
        --registry) REGISTRY="$2"; shift ;;
        --platform) PLATFORM="$2"; shift ;;
        --parallel) PARALLEL=true ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

SERVICES=("api" "gateway" "runtime" "queue" "data-engine" "cli")

build_rust_service() {
    local service=$1
    local dir=$service
    echo "Building Rust service: $service (in dir $dir)..."
    
    # Create a log file for parallel builds to avoid interlaced output
    local log_file="/tmp/build_${service}.log"
    
    {
        if [ "$DOCKER_BUILD" = true ]; then
            if [ -f "$dir/Dockerfile" ]; then
                TAG="fluxbase-$service:latest"
                if [ -n "$REGISTRY" ]; then
                    TAG="$REGISTRY/$service:latest"
                fi
                
                PLATFORM_ARG=""
                if [ -n "$PLATFORM" ]; then
                    PLATFORM_ARG="--platform $PLATFORM"
                fi
                
                PACKAGE_NAME=$service
                if [ "$service" == "queue" ]; then
                    PACKAGE_NAME="fluxbase-queue"
                fi
                
                docker build $PLATFORM_ARG -t "$TAG" -f "$dir/Dockerfile" --build-arg PACKAGE_NAME=$PACKAGE_NAME .
            else
                echo "Warning: No Dockerfile found for $dir, skipping Docker build."
                SQLX_OFFLINE=true cargo build --release -p "$service"
            fi
        else
            # Try to use DATABASE_URL from service's .env if it exists, otherwise use root's if available
            if [ -f "$dir/.env" ]; then
                export $(grep DATABASE_URL "$dir/.env" | xargs)
            elif [ -f "api/.env" ]; then
                export $(grep DATABASE_URL "api/.env" | xargs)
            fi
            
            if [ -n "$DATABASE_URL" ]; then
                cargo build --release -p "$service"
            else
                SQLX_OFFLINE=true cargo build --release -p "$service"
            fi
        fi
    } > "$log_file" 2>&1
    
    local status=$?
    if [ $status -eq 0 ]; then
        echo "✅ $service build successful."
    else
        echo "❌ $service build failed. See $log_file for details."
        cat "$log_file"
        return $status
    fi
}

build_dashboard() {
    echo "Building Dashboard..."
    (
        cd dashboard
        npm install > /tmp/dashboard_install.log 2>&1
        npm run build > /tmp/dashboard_build.log 2>&1
    )
    if [ $? -eq 0 ]; then
        echo "✅ dashboard build successful."
    else
        echo "❌ dashboard build failed. See /tmp/dashboard_build.log"
        return 1
    fi
}

if [ "$SERVICE_NAME" == "all" ]; then
    if [ "$PARALLEL" = true ]; then
        echo "Starting parallel build for all services..."
        for service in "${SERVICES[@]}"; do
            build_rust_service "$service" &
        done
        build_dashboard &
        wait
    else
        for service in "${SERVICES[@]}"; do
            build_rust_service "$service"
        done
        build_dashboard
    fi
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

echo "All builds complete!"
