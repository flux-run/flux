#!/bin/bash

# Fluxbase Deploy Script
# Usage: ./scripts/deploy.sh --env <staging|production> [--service <name>] [--tag <tag>] [--project <id>] [--region <region>] [--dry-run]

set -e

# Default values
ENV=""
DRY_RUN=false
PROJECT_ID="fluxbase-app"
REGION="asia-south1"
REGISTRY=""
SERVICE_NAME="all"
TAG="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --env) ENV="$2"; shift ;;
        --dry-run) DRY_RUN=true ;;
        --project) PROJECT_ID="$2"; shift ;;
        --region) REGION="$2"; shift ;;
        --service) SERVICE_NAME="$2"; shift ;;
        --tag) TAG="$2"; shift ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

if [ -z "$ENV" ]; then
    echo "Error: --env parameter is required (staging|production)"
    exit 1
fi

REGISTRY="${REGION}-docker.pkg.dev/${PROJECT_ID}/fluxbase"

echo "Deploying to $ENV environment (GCP Project: $PROJECT_ID, Region: $REGION, Tag: $TAG)..."

deploy_cloud_run() {
    local service=$1
    local image_tag="${REGISTRY}/${service}:${TAG}"
    local local_tag="fluxbase-${service}:${TAG}"
    
    echo "Pushing image $image_tag to Artifact Registry..."
    if [ "$DRY_RUN" = true ]; then
        echo "[DRY-RUN] Would run: docker push $image_tag"
    else
        # Try to push; if local image exists under local_tag, tag it for registry and push.
        docker push "$image_tag" || {
            echo "Push failed. Re-tagging from $local_tag and retrying..."
            docker tag "$local_tag" "$image_tag"
            docker push "$image_tag"
        }
    fi

    echo "Deploying $service to Cloud Run..."
    local deploy_name="fluxbase-${service}"
    
    # Special case for queue naming if needed, but since we renamed service to 'queue', 
    # fluxbase-queue is correct.
    
    # Match the specific directory for env.yaml
    local dir=$service
    # Service name is 'queue', dir is also 'queue' (mapped in SERVICES)
    
    local env_file="$dir/env.yaml"
    local env_vars=""
    if [ -f "$env_file" ]; then
        env_vars="--env-vars-file $env_file"
    fi

    if [ "$DRY_RUN" = true ]; then
        echo "[DRY-RUN] Would run: gcloud run deploy $deploy_name --image $image_tag --region $REGION --project $PROJECT_ID --platform managed --memory 1Gi --cpu 1 --allow-unauthenticated $env_vars"
    else
        gcloud run deploy "$deploy_name" \
            --image "$image_tag" \
            --region "$REGION" \
            --project "$PROJECT_ID" \
            --platform managed \
            --memory 1Gi \
            --cpu 1 \
            --allow-unauthenticated \
            $env_vars

        local active_image
        local active_rev
        active_image=$(gcloud run services describe "$deploy_name" \
            --region "$REGION" \
            --project "$PROJECT_ID" \
            --format="value(spec.template.spec.containers[0].image)")
        active_rev=$(gcloud run services describe "$deploy_name" \
            --region "$REGION" \
            --project "$PROJECT_ID" \
            --format="value(status.latestReadyRevisionName)")

        echo "✅ $deploy_name revision: $active_rev"
        echo "   image: $active_image"
    fi
}

SERVICES=("api" "gateway" "runtime" "queue" "data-engine")

if [ "$SERVICE_NAME" == "all" ]; then
    for service in "${SERVICES[@]}"; do
        deploy_cloud_run "$service"
    done
else
    if [[ " ${SERVICES[@]} " =~ " ${SERVICE_NAME} " ]]; then
        deploy_cloud_run "$SERVICE_NAME"
    else
        echo "Error: Unknown service $SERVICE_NAME"
        exit 1
    fi
fi

echo "Deployment to $ENV complete!"
