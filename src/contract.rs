#[cfg(not(feature = "library"))]
use cosmwasm_std::{
    attr, entry_point, to_binary, Addr, Binary, Coin, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult, Uint128, WasmMsg,
};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InfoResponse, InstantiateMsg, QueryMsg};
use crate::state::{State, STATE};

use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
// version info for migration info
const CONTRACT_NAME: &str = "crates.io:cosmwasm-16";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        owner: info.sender,
        cw20_address: msg.cw20_address,
        price: Coin {
            denom: msg.denom,
            amount: msg.price,
        },
        balance: Uint128::zero(),
    };
    STATE.save(deps.storage, &state)?;

    Ok(Response::default())
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetInfo {} => to_binary(&query_info(deps)?),
    }
}

fn query_info(deps: Deps) -> StdResult<InfoResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(InfoResponse {
        owner: state.owner,
        cw20_address: state.cw20_address,
        price: state.price,
        balance: state.balance,
    })
}


#[cfg(test)]
mod tests {
  use super::*;
      use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
      use cosmwasm_std::{coins, from_binary, Uint128};

      #[test]
      fn proper_initialization() {
          let mut deps = mock_dependencies(&[]);

          let msg = InstantiateMsg {
              cw20_address: Addr::unchecked("asdf"),
              price: Uint128::from(7u128),
              denom: "token".to_string(),
          };
          let info = mock_info("creator", &coins(1000, "earth"));

          // we can just call .unwrap() to assert this was a success
          let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
          assert_eq!(0, res.messages.len());

          // it worked, let's query the state
          let res = query(deps.as_ref(), mock_env(), QueryMsg::GetInfo {}).unwrap();
          let value: InfoResponse = from_binary(&res).unwrap();
          assert_eq!(Uint128::from(7u128), value.price.amount);
      }
}
