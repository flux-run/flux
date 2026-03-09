#!/bin/bash

# Fluxbase Build Script
# Usage: ./scripts/build.sh [--docker] [--service <name>] [--tag <tag>] [--registry <registry>] [--platform <platform>] [--parallel]

set -e

# Default values
DOCKER_BUILD=false
SERVICE_NAME="all"
REGISTRY=""
PLATFORM=""
PARALLEL=false
TAG="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --docker) DOCKER_BUILD=true ;;
        --service) SERVICE_NAME="$2"; shift ;;
        --tag) TAG="$2"; shift ;;
        --registry) REGISTRY="$2"; shift ;;
        --platform) PLATFORM="$2"; shift ;;
        --parallel) PARALLEL=true ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

SERVICES=("api" "gateway" "runtime" "queue" "data-engine" "cli")

package_name_for_service() {
    local service=$1
    if [ "$service" == "queue" ]; then
        echo "fluxbase-queue"
    else
        echo "$service"
    fi
}

build_rust_service() {
    local service=$1
    local dir=$service
    echo "Building Rust service: $service (in dir $dir)..."
    
    # Create a log file for parallel builds to avoid interlaced output
    local log_file="/tmp/build_${service}.log"
    
    {
        if [ "$DOCKER_BUILD" = true ]; then
            if [ -f "$dir/Dockerfile" ]; then
                local_tag="fluxbase-$service:$TAG"
                if [ -n "$REGISTRY" ]; then
                    image_tag="$REGISTRY/$service:$TAG"
                else
                    image_tag="$local_tag"
                fi
                
                PLATFORM_ARG=""
                if [ -n "$PLATFORM" ]; then
                    PLATFORM_ARG="--platform $PLATFORM"
                fi
                
                PACKAGE_NAME=$(package_name_for_service "$service")
                
                docker build $PLATFORM_ARG \
                    -t "$local_tag" \
                    -f "$dir/Dockerfile" \
                    --build-arg PACKAGE_NAME="$PACKAGE_NAME" \
                    .

                if [ -n "$REGISTRY" ]; then
                    docker tag "$local_tag" "$image_tag"
                    echo "Tagged image: $image_tag"
                else
                    echo "Built image: $local_tag"
                fi
            else
                echo "Warning: No Dockerfile found for $dir, skipping Docker build."
                PACKAGE_NAME=$(package_name_for_service "$service")
                SQLX_OFFLINE=true cargo build --release -p "$PACKAGE_NAME"
            fi
        else
            # Try to use DATABASE_URL from service's .env if it exists, otherwise use root's if available
            if [ -f "$dir/.env" ]; then
                export $(grep DATABASE_URL "$dir/.env" | xargs)
            elif [ -f "api/.env" ]; then
                export $(grep DATABASE_URL "api/.env" | xargs)
            fi
            
            PACKAGE_NAME=$(package_name_for_service "$service")
            if [ -n "${DATABASE_URL:-}" ]; then
                cargo build --release -p "$PACKAGE_NAME"
            else
                SQLX_OFFLINE=true cargo build --release -p "$PACKAGE_NAME"
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
echo "Build tag: $TAG"
