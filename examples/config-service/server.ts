// 基于经过校验的配置构造应用。
export function createApp(config: string[]) {
  return registerRoutes(config);
}

// 为应用注册状态与配置接口。
function registerRoutes(config: string[]) {
  return { config, handler: healthCheck };
}

// 提供服务存活状态。
function healthCheck() {
  return { status: "ok" };
}

// 交付应用实例；示例不实际监听网络端口。
export function startServer(app: object) {
  return app;
}
