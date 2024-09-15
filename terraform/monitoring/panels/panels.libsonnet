local panels = (import '../grafonnet-lib/defaults.libsonnet').panels;
local redis  = panels.aws.redis;

{
  app: {
    handlers_latency:     (import 'app/handlers_latency.libsonnet'        ).new,
    handlers_rate:        (import 'app/handlers_rate.libsonnet'           ).new,
  },

  ecs: {
    availability:         (import 'ecs/availability.libsonnet'            ).new,
    cpu:                  (import 'ecs/cpu.libsonnet'                     ).new,
    memory:               (import 'ecs/memory.libsonnet'                  ).new,
  },

  weights: {
    provider:             (import 'weights/provider.libsonnet'            ).new,
  },

  usage: {
    provider:             (import 'usage/provider.libsonnet'              ).new,
  },

  status: {
    provider:             (import 'status/provider.libsonnet'             ).new,
  },

  proxy: {
    calls:                  (import 'proxy/calls.libsonnet'                 ).new,
    latency:                (import 'proxy/latency.libsonnet'               ).new,
    errors_non_provider:    (import 'proxy/errors_non_provider.libsonnet'   ).new,
    errors_provider:        (import 'proxy/errors_provider.libsonnet'       ).new,
    provider_retries:       (import 'proxy/rpc_retries.libsonnet'           ).new,
    rate_limited_counter:   (import 'proxy/rate_limited_counter.libsonnet'  ).new,
    http_codes:             (import 'proxy/http_codes.libsonnet'            ).new,
  },

  projects: {
    rejected_projects:      (import 'projects/rejected_projects.libsonnet'     ).new,
    quota_limited_projects: (import 'projects/quota_limited_projects.libsonnet').new,
    cache_latency:          (import 'projects/cache_latency.libsonnet'         ).new,
    fetch_latency:          (import 'projects/fetch_latency.libsonnet'         ).new,
  },

  rate_limiting: {
    counter:      (import 'rate_limiting/counter.libsonnet'     ).new,
    latency:      (import 'rate_limiting/latency.libsonnet'     ).new,
    rate_limited: (import 'rate_limiting/rate_limited.libsonnet').new,
  },

  redis: {
    cpu(ds, vars):            redis.cpu.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.redis_cluster_id),
    memory(ds, vars):         redis.memory.panel(ds.cloudwatch, vars.namespace, vars.environment, vars.notifications, vars.redis_cluster_id),
  },

  identity: {
    requests:             (import 'identity/requests.libsonnet'           ).new,
    availability:         (import 'identity/availability.libsonnet'       ).new,
    latency:              (import 'identity/latency.libsonnet'            ).new,
    cache:                (import 'identity/cache.libsonnet'              ).new,
    usage:                (import 'identity/usage.libsonnet'              ).new,
  },

  names: {
    registered:           (import 'names/registered.libsonnet'            ).new,
  },

  history: {
    availability:         (import 'history/availability.libsonnet'        ).new,
    requests:             (import 'history/requests.libsonnet'            ).new,
    latency:              (import 'history/latency.libsonnet'             ).new,
  },

  lb: {
    active_connections:       (import 'lb/active_connections.libsonnet'         ).new,
    error_4xx:                (import 'lb/error_4xx.libsonnet'                  ).new,
    error_5xx:                (import 'lb/error_5xx.libsonnet'                  ).new,
    error_5xx_logs:           (import 'lb/error_5xx_logs.libsonnet'             ).new,
    healthy_hosts:            (import 'lb/healthy_hosts.libsonnet'              ).new,
    requests:                 (import 'lb/requests.libsonnet'                   ).new,
    response_time:            (import 'lb/latency.libsonnet'                    ).new, 
  },

  irn: {
    latency: (import 'irn/latency.libsonnet').new,
  },

  non_rpc: {
    endpoints_latency: (import 'non_rpc/endpoints_latency.libsonnet').new,
  },
}
