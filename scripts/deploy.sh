#!/bin/bash

# Fluxbase Deploy Script
# Usage: ./scripts/deploy.sh --env <staging|production>

set -e

# Default values
ENV=""
DRY_RUN=false

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --env) ENV="$2"; shift ;;
        --dry-run) DRY_RUN=true ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

if [ -z "$ENV" ]; then
    echo "Error: --env parameter is required (staging|production)"
    exit 1
fi

echo "Deploying to $ENV environment..."

deploy_service() {
    local service=$1
    echo "Deploying $service..."
    if [ "$DRY_RUN" = true ]; then
        echo "[DRY-RUN] Would deploy $service to $ENV"
    else
        # Placeholder for actual deployment logic (e.g., docker push, fly deploy, kubectl apply)
        echo "Successfully deployed $service to $ENV"
    fi
}

SERVICES=("api" "gateway" "runtime" "fluxbase-queue" "dashboard")

for service in "${SERVICES[@]}"; do
    deploy_service "$service"
done

echo "Deployment to $ENV complete!"
