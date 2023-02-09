import React, { useContext } from 'react'
export type HealthyJSON = { [element: string]: string }

export type FaktsEndpoint = {
  name: string
  base_url: string
}

export type Fakts = any

export type FaktsContext = {
  fakts?: Fakts
  endpoint?: FaktsEndpoint
  changeFakts: (fakts: Fakts | null) => void
  load: (endpoint: FaktsEndpoint) => void
}

export const FaktsContext = React.createContext<FaktsContext>({
  load: () => null as unknown as Promise<Fakts>,
  changeFakts: () => {},
})

export const useFakts = () => useContext(FaktsContext)
