# frozen_string_literal: true

module Tracetells
  # In Rails: configures from the environment, adds the Rack middleware, and
  # subscribes to Rails.error, which sees unhandled errors in controllers and
  # jobs (Rails 7+). Handled errors reported with `handled: true` are left out
  # unless their severity is :error, and so are errors Rails answers with a 4xx.
  class Railtie < ::Rails::Railtie
    initializer "tracetells.middleware" do |app|
      Tracetells.configure unless Tracetells.client
      app.middleware.insert_before(0, Tracetells::Rack)
    end

    config.after_initialize do
      Rails.error.subscribe(Tracetells::Railtie::Subscriber.new) if Rails.respond_to?(:error)
    end

    class Subscriber
      def report(error, handled:, severity:, context:, source: nil)
        return if handled && severity != :error

        env = Thread.current[:tracetells_env]
        status = status_for(error)
        # 404s and other client errors (RecordNotFound, RoutingError) aren't server errors.
        return if env && status < 500

        request = env ? Tracetells::Rack.request_info(env, status: status) : nil
        env["tracetells.reported"] = true if env
        Tracetells.report(error, request: request)
      end

      private

      def status_for(error)
        if defined?(ActionDispatch::ExceptionWrapper)
          ActionDispatch::ExceptionWrapper.status_code_for_exception(error.class.name)
        else
          500
        end
      end
    end
  end
end
