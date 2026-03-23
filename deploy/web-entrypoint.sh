#!/bin/sh
# Extract hostname from API_URL for the Host header
export API_HOST=$(echo "$API_URL" | sed 's|https\?://||' | sed 's|/.*||')
envsubst '${API_URL} ${API_HOST}' < /etc/nginx/conf.d/default.conf.template > /etc/nginx/conf.d/default.conf
exec nginx -g 'daemon off;'
