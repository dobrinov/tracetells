# frozen_string_literal: true

# A real Rails app, in its own process (`rake test:rails`): the Railtie adds the
# middleware and subscribes to Rails.error.
require "helper"

TRACE = FakeTrace.new
ENV["TRACE_SERVER_KEY"] = "tse_test"
ENV["TRACE_ENDPOINT"] = "http://127.0.0.1:#{TRACE.port}"
ENV["TRACE_RELEASE"] = "v9"

require "rails"
require "action_controller/railtie"
require "tracetells"

class TestApp < Rails::Application
  config.root = __dir__
  config.eager_load = false
  config.secret_key_base = "x" * 64
  config.hosts.clear
  config.logger = Logger.new(nil)
  config.action_dispatch.show_exceptions = :all
  config.consider_all_requests_local = false
  routes.append do
    get "/orders/:id" => "orders#show"
    get "/missing/:id" => "orders#missing"
  end
end

class OrdersController < ActionController::Base
  def show
    nil.total_for("anna@example.com") # NoMethodError
  end

  def missing
    raise ActionController::RoutingError, "nothing at 12345678"
  end
end

TestApp.initialize!

class RailsTest < Minitest::Test
  def test_controller_errors_are_reported_with_their_route
    get = ->(path, headers = {}) { Rails.application.call(Rack::MockRequest.env_for(path, headers)) }
    status, = get.("/orders/8817?coupon=SECRETCOUPON", "HTTP_COOKIE" => "session=SECRETCOOKIE")
    assert_equal 500, status
    status, = get.("/missing/1")
    assert_equal 404, status
    Tracetells.flush(3)

    errors = TRACE.errors
    assert_equal ["NoMethodError"], errors.map { |e| e["class"] }, "one report per error, and no 404s"
    e = errors.first
    assert_equal ["/orders/:id", "GET", 500, "v9"], [e["route"], e["method"], e["status"], e["release"]]
    assert_includes e["message"], "total_for"
    assert(e["backtrace"].first["in_app"], e["backtrace"].first.inspect)
    everything = JSON.generate(TRACE.received.map { |r| r[:body] })
    %w[SECRETCOUPON SECRETCOOKIE anna@example.com].each { |s| refute_includes everything, s }
  end
end
