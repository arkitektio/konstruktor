import React, { useContext } from 'react'

export interface CommunicationContextType {
  call<Options extends {}, Result extends {}>(
    channel: string,
    options?: Options
  ): Promise<Result>
}

export const CommunicationContext =
  React.createContext<CommunicationContextType>({
    call: null as unknown as any,
  })

export const useCommunication = () => useContext(CommunicationContext)
