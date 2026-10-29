# frozen_string_literal: true

# Report a Ruby server's errors to Trace (https://tracetells.com).
#
#   Tracetells.configure # TRACE_SERVER_KEY, TRACE_ENDPOINT, TRACE_ENVIRONMENT, TRACE_RELEASE
#   use Tracetells::Rack # in config.ru; Rails apps get it, and Rails.error, on their own
#
# What is sent: the error's class, its message, the backtrace (files relative
# to your project, lines, methods), the route template, the HTTP method and
# status, the environment, the release, the Ruby version, when and how often.
# Never sent: request or response bodies, params, query strings, headers,
# cookies, sessions, environment variables, source code, host names or who was
# signed in. There is no option to send them. Messages and paths are scrubbed first.

require "json"
require "net/http"
require "uri"
require "rbconfig"
require_relative "tracetells/version"
require_relative "tracetells/scrub"
require_relative "tracetells/client"
require_relative "tracetells/rack"
require_relative "tracetells/railtie" if defined?(::Rails::Railtie)

module Tracetells
  class << self
    # Starts the client. Without a key it does nothing. Options override the environment.
    def configure(key: nil, endpoint: nil, environment: nil, release: nil)
      env = ->(k) { (v = ENV[k].to_s.strip).empty? ? nil : v }
      key ||= env.("TRACE_SERVER_KEY")
      return @client = nil unless key

      @client&.shutdown
      @client = Client.new(
        key: key,
        endpoint: (endpoint || env.("TRACE_ENDPOINT") || "https://app.tracetells.com").sub(%r{/+\z}, ""),
        environment: environment || env.("TRACE_ENVIRONMENT") || env.("RAILS_ENV") || env.("RACK_ENV") || "production",
        release: release || %w[TRACE_RELEASE SOURCE_VERSION HEROKU_SLUG_COMMIT RENDER_GIT_COMMIT GIT_COMMIT GITHUB_SHA].lazy.map { |k| env.(k) }.find(&:itself) || ""
      )
    end

    def client
      @client
    end

    # Reports an error you handled. `request` takes :route, :method and :status.
    def report(error, request: nil)
      @client&.report(error, request || current_request)
    rescue StandardError
      nil
    end

    def report_message(class_name, message, request: nil)
      @client&.report_message(class_name, message, request || current_request)
    rescue StandardError
      nil
    end

    # Sends what's queued, waiting at most `timeout` seconds.
    def flush(timeout = 2)
      @client&.flush(timeout)
    end

    # The request this thread is handling, as Tracetells::Rack saw it.
    def current_request
      env = Thread.current[:tracetells_env]
      env && Tracetells::Rack.request_info(env)
    end
  end
end

at_exit { Tracetells.flush(2) }
