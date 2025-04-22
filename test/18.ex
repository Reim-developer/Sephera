# Test codeLoc is working in Elixir

defmodule Greeter do
  @moduledoc """
  ...
  """

  @doc """
  Prints a hello message.

  ## Parameters

    - name: String that represents the name of the person.

  ## Examples

      iex> Greeter.hello("Sean")
      "Hello, Sean"

      iex> Greeter.hello("pete")
      "Hello, pete"

  """
  @spec hello(String.t()) :: String.t()
  def hello(name) do
    "Hello, " <> name
  end
end