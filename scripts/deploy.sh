#!/bin/bash

# Fluxbase Deploy Script
# Usage: ./scripts/deploy.sh --env <staging|production> [--service <name>] [--tag <tag>] [--project <id>] [--region <region>] [--dry-run]
# Usage: ./scripts/deploy.sh --rollback [--service <name>] [--project <id>] [--region <region>] [--dry-run]

set -e

# Default values
ENV=""
DRY_RUN=false
PROJECT_ID="fluxbase-app"
REGION="asia-south1"
REGISTRY=""
SERVICE_NAME="all"
TAG="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
PLATFORM="linux/amd64"
ROLLBACK=false

# Parse arguments
while [[ "$#" -gt 0 ]]; do
    case $1 in
        --env) ENV="$2"; shift ;;
        --dry-run) DRY_RUN=true ;;
        --rollback) ROLLBACK=true ;;
        --project) PROJECT_ID="$2"; shift ;;
        --region) REGION="$2"; shift ;;
        --service) SERVICE_NAME="$2"; shift ;;
        --tag) TAG="$2"; shift ;;
        --platform) PLATFORM="$2"; shift ;;
        *) echo "Unknown parameter passed: $1"; exit 1 ;;
    esac
    shift
done

if [ "$ROLLBACK" = false ] && [ -z "$ENV" ]; then
    echo "Error: --env parameter is required for deploy (staging|production)"
    echo "       Use --rollback to roll back the last deployment without --env"
    exit 1
fi

REGISTRY="${REGION}-docker.pkg.dev/${PROJECT_ID}/fluxbase"

if [ "$ROLLBACK" = true ]; then
    echo "Rolling back services (GCP Project: $PROJECT_ID, Region: $REGION)..."
else
    echo "Deploying to $ENV environment (GCP Project: $PROJECT_ID, Region: $REGION, Tag: $TAG, Platform: $PLATFORM)..."
fi

package_name_for_service() {
    local service=$1
    if [ "$service" == "queue" ]; then
        echo "fluxbase-queue"
    else
        echo "$service"
    fi
}

ensure_pushed_image() {
    local service=$1
    local image_tag=$2
    local dir=$service
    local package_name
    package_name=$(package_name_for_service "$service")

    # First try push directly (image might already exist locally under registry tag)
    if docker push "$image_tag"; then
        return 0
    fi

    echo "Push failed for $image_tag. Building and pushing $PLATFORM image via buildx..."
    docker buildx build \
        --platform "$PLATFORM" \
        -t "$image_tag" \
        -f "$dir/Dockerfile" \
        --build-arg PACKAGE_NAME="$package_name" \
        --push \
        .
}

deploy_cloud_run() {
    local service=$1
    local image_tag="${REGISTRY}/${service}:${TAG}"
    
    echo "Pushing image $image_tag to Artifact Registry..."
    if [ "$DRY_RUN" = true ]; then
        echo "[DRY-RUN] Would run: docker push $image_tag"
        echo "[DRY-RUN] If missing, would run: docker buildx build --platform $PLATFORM -t $image_tag -f $service/Dockerfile --build-arg PACKAGE_NAME=$(package_name_for_service "$service") --push ."
    else
        ensure_pushed_image "$service" "$image_tag"
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

SERVICES=("api" "runtime" "queue" "server")

# ── Rollback ──────────────────────────────────────────────────────────────────
#
# Rolls back a Cloud Run service to its previously active revision by setting
# 100% of traffic to the PREVIOUS revision tag.
#
# Cloud Run keeps the last N revisions around; PREVIOUS is always the one that
# was serving traffic before the most recent deploy.

rollback_cloud_run() {
    local service=$1
    local deploy_name="fluxbase-${service}"

    echo "Rolling back $deploy_name…"

    # Fetch the two most-recent ready revisions (newest first).
    local revisions
    revisions=$(gcloud run revisions list \
        --service "$deploy_name" \
        --region "$REGION" \
        --project "$PROJECT_ID" \
        --format="value(metadata.name)" \
        --sort-by="~metadata.creationTimestamp" \
        --limit=2 2>/dev/null)

    local current_rev
    local previous_rev
    current_rev=$(echo "$revisions"  | sed -n '1p')
    previous_rev=$(echo "$revisions" | sed -n '2p')

    if [ -z "$previous_rev" ]; then
        echo "⚠️  No previous revision found for $deploy_name — skipping."
        return 0
    fi

    echo "  current:  $current_rev"
    echo "  previous: $previous_rev"

    if [ "$DRY_RUN" = true ]; then
        echo "[DRY-RUN] Would run: gcloud run services update-traffic $deploy_name --to-revisions=$previous_rev=100 --region $REGION --project $PROJECT_ID"
    else
        gcloud run services update-traffic "$deploy_name" \
            --to-revisions="${previous_rev}=100" \
            --region "$REGION" \
            --project "$PROJECT_ID"
        echo "✅ $deploy_name rolled back to $previous_rev"
    fi
}

# ── Dispatch ──────────────────────────────────────────────────────────────────

run_for_services() {
    local fn=$1
    if [ "$SERVICE_NAME" == "all" ]; then
        for service in "${SERVICES[@]}"; do
            $fn "$service"
        done
    else
        if [[ " ${SERVICES[*]} " =~ " ${SERVICE_NAME} " ]]; then
            $fn "$SERVICE_NAME"
        else
            echo "Error: Unknown service '$SERVICE_NAME'. Valid: ${SERVICES[*]}"
            exit 1
        fi
    fi
}

if [ "$ROLLBACK" = true ]; then
    run_for_services rollback_cloud_run
    echo "Rollback complete!"
else
    run_for_services deploy_cloud_run
    echo "Deployment to $ENV complete!"
fi
