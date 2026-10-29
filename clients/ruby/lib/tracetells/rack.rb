# frozen_string_literal: true

module Tracetells
  # Rack middleware: reports exceptions raised while handling a request, with
  # its route, method and status, and re-raises them so the app's own error
  # handling is unchanged. In Rails the Railtie adds it.
  class Rack
    def initialize(app)
      @app = app
    end

    def call(env)
      previous = Thread.current[:tracetells_env]
      Thread.current[:tracetells_env] = env
      @app.call(env)
    rescue Exception => e # rubocop:disable Lint/RescueException
      Tracetells.report(e, request: self.class.request_info(env, status: 500)) unless env["tracetells.reported"]
      raise
    ensure
      Thread.current[:tracetells_env] = previous
    end

    # The route template when the framework knows it (Rails 7.1+, Sinatra), else
    # the path, which is scrubbed of ids before it's sent. Nothing else of the request.
    def self.request_info(env, status: nil)
      route = env["action_dispatch.route_uri_pattern"] || env["sinatra.route"]&.split(" ", 2)&.last || env["PATH_INFO"].to_s
      { route: route.to_s.sub(/\(\.:format\)\z/, ""), method: env["REQUEST_METHOD"], status: status }
    end
  end
end
