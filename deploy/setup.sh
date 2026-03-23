#!/bin/bash
# MineGraph GCP Setup Reference Script
# Run these commands step by step — don't execute this file blindly.
# Replace placeholders: PROJECT_ID, DB_PASSWORD, REGION

set -euo pipefail

PROJECT_ID="your-project-id"
REGION="us-central1"
DB_PASSWORD="change-me"

echo "=== Step 1: Enable APIs ==="
gcloud config set project "$PROJECT_ID"
gcloud services enable \
  run.googleapis.com \
  sqladmin.googleapis.com \
  cloudbuild.googleapis.com \
  secretmanager.googleapis.com \
  artifactregistry.googleapis.com

echo "=== Step 2: Artifact Registry ==="
gcloud artifacts repositories create minegraph \
  --repository-format=docker \
  --location="$REGION" \
  || echo "Already exists"

echo "=== Step 3: Cloud SQL ==="
gcloud sql instances create minegraph \
  --database-version=POSTGRES_16 \
  --tier=db-f1-micro \
  --region="$REGION" \
  --root-password="$DB_PASSWORD"

gcloud sql databases create minegraph --instance=minegraph
gcloud sql users create minegraph --instance=minegraph --password="$DB_PASSWORD"

echo "=== Step 4: Server Key ==="
echo "Generate key locally first:"
echo "  cargo run -p minegraph-cli -- keygen --name minegraph-server -o server-key.json"
echo "Then upload:"
echo "  gcloud secrets create minegraph-server-key --data-file=server-key.json"
echo "  rm server-key.json"

echo "=== Step 5: Cloud Build Permissions ==="
PROJECT_NUM=$(gcloud projects describe "$PROJECT_ID" --format='value(projectNumber)')

gcloud projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:${PROJECT_NUM}@cloudbuild.gserviceaccount.com" \
  --role="roles/run.admin"

gcloud iam service-accounts add-iam-policy-binding \
  "${PROJECT_NUM}-compute@developer.gserviceaccount.com" \
  --member="serviceAccount:${PROJECT_NUM}@cloudbuild.gserviceaccount.com" \
  --role="roles/iam.serviceAccountUser"

echo "=== Step 6: Build & Deploy Server ==="
gcloud builds submit \
  --tag "$REGION-docker.pkg.dev/$PROJECT_ID/minegraph/server" \
  --timeout=600s

gcloud run deploy minegraph-server \
  --image "$REGION-docker.pkg.dev/$PROJECT_ID/minegraph/server" \
  --region "$REGION" --allow-unauthenticated \
  --add-cloudsql-instances "$PROJECT_ID:$REGION:minegraph" \
  --set-env-vars "\
DATABASE_URL=postgres://minegraph:${DB_PASSWORD}@/minegraph?host=/cloudsql/${PROJECT_ID}:${REGION}:minegraph,\
ALLOWED_ORIGINS=https://minegraph.net,\
DB_MAX_CONNECTIONS=5,LEADERBOARD_CAPACITY=500,MAX_N=62" \
  --set-secrets "SERVER_KEY_PATH=/secrets/key.json:minegraph-server-key:latest" \
  --min-instances 1 --max-instances 4 \
  --cpu 2 --memory 1Gi --concurrency 50 --timeout 60s \
  --port 3001 --command minegraph-server --args "--migrate"

echo "=== Step 7: Build & Deploy Web App ==="
gcloud builds submit \
  --tag "$REGION-docker.pkg.dev/$PROJECT_ID/minegraph/web" \
  -f Dockerfile.web --timeout=300s

gcloud run deploy minegraph-web \
  --image "$REGION-docker.pkg.dev/$PROJECT_ID/minegraph/web" \
  --region "$REGION" --allow-unauthenticated \
  --set-env-vars "API_URL=https://api.minegraph.net" \
  --min-instances 0 --max-instances 2 \
  --cpu 1 --memory 256Mi --concurrency 100 --timeout 30s \
  --port 8080

echo "=== Step 8: Build & Deploy Dashboard ==="
gcloud builds submit \
  --tag "$REGION-docker.pkg.dev/$PROJECT_ID/minegraph/dashboard" \
  -f Dockerfile.dashboard --timeout=600s

gcloud run deploy minegraph-dashboard \
  --image "$REGION-docker.pkg.dev/$PROJECT_ID/minegraph/dashboard" \
  --region "$REGION" --allow-unauthenticated \
  --min-instances 1 --max-instances 1 \
  --cpu 1 --memory 512Mi --concurrency 200 \
  --timeout 3600s --port 4000

echo "=== Step 9: Custom Domains ==="
gcloud run domain-mappings create --service=minegraph-server \
  --domain=api.minegraph.net --region="$REGION"
gcloud run domain-mappings create --service=minegraph-web \
  --domain=minegraph.net --region="$REGION"
gcloud run domain-mappings create --service=minegraph-dashboard \
  --domain=dashboard.minegraph.net --region="$REGION"

echo ""
echo "=== DNS Configuration (do this in Wix) ==="
echo "Add these DNS records in Wix domain management:"
echo ""
echo "  CNAME  api        -> ghs.googlehosted.com."
echo "  CNAME  dashboard  -> ghs.googlehosted.com."
echo "  A      @          -> (get IPs from: gcloud run domain-mappings describe --domain=minegraph.net --region=$REGION)"
echo ""
echo "HTTPS certificates are provisioned automatically by Cloud Run after DNS propagation."
