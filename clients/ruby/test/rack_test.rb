# frozen_string_literal: true

require "helper"
require "rack"
require "rack/mock"
require "tracetells"

class RackTest < Minitest::Test
  SECRETS = %w[SECRETCOUPON SECRETCOOKIE SECRETBEARER SECRETBODY 4111 anna@example.com sk_live /Users/ /home/].freeze

  def setup
    @trace = FakeTrace.new
    Tracetells.configure(key: "tse_test", endpoint: "http://127.0.0.1:#{@trace.port}", environment: "test", release: "v1.2.3")
  end

  def teardown
    Tracetells.client&.shutdown
    @trace.close
  end

  def test_reports_exceptions_without_request_data
    app = Tracetells::Rack.new(->(_env) { raise ArgumentError, "card 4111 1111 1111 1111 for anna@example.com" })
    env = Rack::MockRequest.env_for("/orders/8817?coupon=SECRETCOUPON", method: "POST", input: "password=SECRETBODY",
                                    "HTTP_COOKIE" => "session=SECRETCOOKIE", "HTTP_AUTHORIZATION" => "Bearer SECRETBEARER")
    assert_raises(ArgumentError) { app.call(env) }

    50.times { Tracetells.report(RangeError.new("storm")) }
    begin
      raise IOError, "token sk_live_51HkT9a2eZvKYlo2C0jw8Ln is invalid"
    rescue IOError => e
      Tracetells.report(e)
    end
    Tracetells.flush(3)

    assert(@trace.received.all? { |r| r[:auth] == "Bearer tse_test" })
    e = @trace.errors.find { |x| x["class"] == "ArgumentError" }
    assert_equal ["/orders/:id", "POST", 500], [e["route"], e["method"], e["status"]]
    assert_equal "card [card] for [email]", e["message"]
    assert_equal %w[test v1.2.3 ruby], [e["environment"], e["release"], e["runtime"]["name"]]
    assert(e["backtrace"].any? { |f| f["in_app"] && f["file"].start_with?("test/") }, e["backtrace"].inspect)

    assert_equal [1, 49], @trace.errors.select { |x| x["class"] == "RangeError" }.map { |x| x["count"] }
    assert_equal "token [token] is invalid", @trace.errors.find { |x| x["class"] == "IOError" }["message"]

    everything = JSON.generate(@trace.received.map { |r| r[:body] })
    SECRETS.each { |s| refute_includes everything, s }
    %w[headers cookies params body user context session].each { |k| assert(@trace.errors.none? { |x| x.key?(k) }, "a #{k} field was sent") }
  end

  def test_does_nothing_without_a_key
    Tracetells.configure(key: nil)
    ENV.delete("TRACE_SERVER_KEY")
    Tracetells.configure
    assert_nil Tracetells.client
    Tracetells.report(RuntimeError.new("nowhere"))
  end
end
