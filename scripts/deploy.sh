#!/bin/bash

# Fluxbase Deploy Script
# Usage: ./scripts/deploy.sh --env <staging|production>

set -e

# Default values
ENV=""
DRY_RUN=false
PROJECT_ID="fluxbase-app"
REGION="asia-south1"
REGISTRY="${REGION}-docker.pkg.dev/${PROJECT_ID}/fluxbase"

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --env) ENV="$2"; shift ;;
        --dry-run) DRY_RUN=true ;;
        --project) PROJECT_ID="$2"; shift ;;
        --region) REGION="$2"; shift ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

if [ -z "$ENV" ]; then
    echo "Error: --env parameter is required (staging|production)"
    exit 1
fi

echo "Deploying to $ENV environment (GCP Project: $PROJECT_ID, Region: $REGION)..."

deploy_cloud_run() {
    local service=$1
    local image_tag="${REGISTRY}/${service}:latest"
    
    echo "Pushing image $image_tag to Artifact Registry..."
    if [ "$DRY_RUN" = true ]; then
        echo "[DRY-RUN] Would run: docker push $image_tag"
    else
        # Try to push, if it fails because it's not tagged correctly, tag and push
        docker push "$image_tag" || {
            echo "Push failed. Re-tagging and retrying..."
            docker tag "fluxbase-${service}:latest" "$image_tag"
            docker push "$image_tag"
        }
    fi

    echo "Deploying $service to Cloud Run..."
    local deploy_name="fluxbase-${service}-${ENV}"
    # Match the specific directory for env.yaml
    local dir=$service
    if [ "$service" == "fluxbase-queue" ]; then dir="queue"; fi
    
    local env_file="$dir/env.yaml"
    local env_vars=""
    if [ -f "$env_file" ]; then
        env_vars="--env-vars-file $env_file"
    fi

    if [ "$DRY_RUN" = true ]; then
        echo "[DRY-RUN] Would run: gcloud run deploy $deploy_name --image $image_tag --region $REGION --project $PROJECT_ID --platform managed --allow-unauthenticated $env_vars"
    else
        gcloud run deploy "$deploy_name" \
            --image "$image_tag" \
            --region "$REGION" \
            --project "$PROJECT_ID" \
            --platform managed \
            --allow-unauthenticated \
            $env_vars
    fi
}

SERVICES=("api" "gateway" "runtime" "fluxbase-queue" "data-engine")

for service in "${SERVICES[@]}"; do
    deploy_cloud_run "$service"
done

echo "Deployment to $ENV complete!"
