import React, { useEffect, useState } from 'react'
import { FaktsContext, Fakts, FaktsEndpoint } from './fakts-config'

const window_host = window.location.hostname
console.log(`Is hosted on ${window_host}`)

export type FaktsProps = {
  children?: any
  endpoint?: FaktsEndpoint
  store?: string
  clientId: string
  clientSecret: string
}

export const FaktsProvider: React.FC<FaktsProps> = ({
  children,
  endpoint,
  clientId,
  clientSecret,
  store = 'fakts-config',
}) => {
  const [faktsState, setConfigState] = useState<any | null>(null)
  const [faktsEndpoint, setFaktsEndpoint] = useState<FaktsEndpoint | undefined>(
    endpoint
  )

  const changeFakts = (configState?: Fakts | undefined) => {
    setConfigState(configState)
    localStorage.setItem(store, configState ? JSON.stringify(configState) : '')
  }

  useEffect(() => {
    if (endpoint) {
      fetch(`${endpoint.base_url}claim/`, {
        method: 'POST',
        body: JSON.stringify({
          headers: {
            'Content-Type': 'application/json',
          },
          client_secret: clientSecret,
          client_id: clientId,
          scopes: ['read', 'write'],
        }),
      })
        .then((res) => {
          console.log(res)
          return res.json()
        })
        .then((res) => {
          console.log(res)
          changeFakts(res.config)
          return res.config
        })
        .catch((e) => {
          console.error(e)
          changeFakts()
        })
    }
  }, [endpoint])

  const load = (endpoint: FaktsEndpoint) => {
    setFaktsEndpoint(endpoint)
  }

  useEffect(() => {
    const value = localStorage.getItem(store)

    if (value) {
      setConfigState(JSON.parse(value))
    }
  }, [store])

  return (
    <FaktsContext.Provider
      value={{
        fakts: faktsState,
        changeFakts: changeFakts,
        load: load,
      }}
    >
      {children}
    </FaktsContext.Provider>
  )
}
