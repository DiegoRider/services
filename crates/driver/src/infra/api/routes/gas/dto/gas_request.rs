use {
    crate::{
        domain::{
            competition::{Auction, Solution},
            eth,
            liquidity::{
                self,
                balancer,
                uniswap::{
                    self,
                    v3::{Liquidity as V3Liquidity, LiquidityNet, SqrtPrice, Tick},
                },
                // zeroex::{self, Amounts},
                Kind,
            },
            Liquidity,
        },
        infra::{
            api::{routes::solve::Auction as DTOAuction, State},
            observe,
            solver::dto::{Liquidity as DTOLiquidity, Solutions, WeightedProductVersion},
            Solver,
        },
        util::conv::u256::U256Ext,
    },
    // ethcontract::H160,
    num::rational::Ratio,
    serde::Deserialize,
    serde_with::serde_as,
};

#[serde_as]
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GasRequest {
    pub solutions: Solutions,
    pub auction: DTOAuction,
    pub liquidity: Vec<DTOLiquidity>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("error parsing liquidity")]
    LiquidityError,
    #[error("error parsing solutions")]
    EncodeSolutionError,
    #[error("error parsing auction")]
    EncodeAuctionError,
}

impl GasRequest {
    pub async fn into_domain(
        self,
        state: &State,
        weth: eth::WethAddress,
        solver: Solver,
    ) -> Result<(Auction, Vec<Solution>), Error> {
        // todo: pass this in the solve request??
        let vault: eth::H160 = "0xba12222222228d8ba445958a75a0704d566bf2c8"
            .parse()
            .unwrap();

        let liquidity = self
            .liquidity
            .into_iter()
            .map(|l| match l {
                DTOLiquidity::ConstantProduct(dto_pool) => {
                    let reserve_assets: Vec<eth::Asset> = dto_pool
                        .tokens
                        .into_iter()
                        .map(|(t, res)| eth::Asset {
                            token: t.into(),
                            amount: res.balance.into(),
                        })
                        .collect();
                    let reserves = uniswap::v2::Reserves::new(
                        reserve_assets[0].clone(),
                        reserve_assets[1].clone(),
                    )
                    .map_err(|err| {
                        observe::invalid_dto(&err, "constant product reserves");
                        Error::LiquidityError
                    })?;
                    Ok(Liquidity {
                        id: dto_pool.id.into(),
                        gas: dto_pool.gas_estimate.into(),
                        kind: Kind::UniswapV2(uniswap::v2::Pool {
                            address: dto_pool.address.into(),
                            router: dto_pool.router.into(),
                            reserves,
                        }),
                    })
                }
                DTOLiquidity::WeightedProduct(dto_pool) => {
                    let reserves = balancer::v2::weighted::Reserves::new(
                        dto_pool
                            .tokens
                            .into_iter()
                            .map(|(t, r)| {
                                Ok(balancer::v2::weighted::Reserve {
                                    asset: eth::Asset {
                                        token: t.into(),
                                        amount: r.balance.into(),
                                    },
                                    weight: decimal_to_weight(r.weight),
                                    scale: decimal_to_scaling_factor(r.scaling_factor)?,
                                })
                            })
                            .collect::<Result<Vec<_>, Error>>()?,
                    )
                    .map_err(|err| {
                        observe::invalid_dto(&err, "weighted product reserves");
                        Error::LiquidityError
                    })?;
                    Ok(Liquidity {
                        id: dto_pool.id.into(),
                        gas: dto_pool.gas_estimate.into(),
                        kind: Kind::BalancerV2Weighted(balancer::v2::weighted::Pool {
                            vault: vault.into(),
                            id: dto_pool.balancer_pool_id.into(),
                            reserves,
                            fee: decimal_to_fee(dto_pool.fee)?,
                            version: match dto_pool.version {
                                WeightedProductVersion::V0 => {
                                    liquidity::balancer::v2::weighted::Version::V0
                                }
                                WeightedProductVersion::V3Plus => {
                                    liquidity::balancer::v2::weighted::Version::V3Plus
                                }
                            },
                        }),
                    })
                }
                DTOLiquidity::Stable(dto_pool) => {
                    let reserves = balancer::v2::stable::Reserves::new(
                        dto_pool
                            .tokens
                            .into_iter()
                            .map(|(t, r)| {
                                Ok(balancer::v2::stable::Reserve {
                                    asset: eth::Asset {
                                        token: t.into(),
                                        amount: r.balance.into(),
                                    },
                                    scale: decimal_to_scaling_factor(r.scaling_factor)?,
                                })
                            })
                            .collect::<Result<Vec<_>, Error>>()?,
                    )
                    .map_err(|err| {
                        observe::invalid_dto(&err, "weighted product reserves");
                        Error::LiquidityError
                    })?;
                    Ok(Liquidity {
                        id: dto_pool.id.into(),
                        gas: dto_pool.gas_estimate.into(),
                        kind: Kind::BalancerV2Stable(balancer::v2::stable::Pool {
                            vault: vault.into(),
                            id: dto_pool.balancer_pool_id.into(), // missing
                            reserves,
                            fee: decimal_to_fee(dto_pool.fee)?,
                            amplification_parameter: decimal_to_amp_param(
                                dto_pool.amplification_parameter,
                            )?,
                        }),
                    })
                }
                DTOLiquidity::ConcentratedLiquidity(dto_pool) => Ok(Liquidity {
                    id: dto_pool.id.into(),
                    gas: dto_pool.gas_estimate.into(),
                    kind: Kind::UniswapV3(uniswap::v3::Pool {
                        router: dto_pool.router.into(),
                        address: dto_pool.address.into(),
                        tokens: liquidity::TokenPair::new(
                            dto_pool.tokens[0].into(),
                            dto_pool.tokens[1].into(),
                        )
                        .map_err(|_| Error::LiquidityError)?,
                        sqrt_price: SqrtPrice(dto_pool.sqrt_price),
                        liquidity: V3Liquidity(dto_pool.liquidity),
                        tick: Tick(dto_pool.tick),
                        liquidity_net: dto_pool
                            .liquidity_net
                            .into_iter()
                            .map(|(k, v)| (Tick(k), LiquidityNet(v)))
                            .collect(),
                        fee: decimal_to_v3_fee(dto_pool.fee)?,
                    }),
                }),
                DTOLiquidity::LimitOrder(_) => {
                    let my_error = Error::LiquidityError;
                    observe::invalid_dto(&my_error, "zeroex limit orders not implemented");
                    return Err(my_error);
                }
            })
            .collect::<Result<Vec<Liquidity>, Error>>()?;

        let auction = self
            .auction
            .into_domain(state.eth(), state.tokens(), state.timeouts())
            .await
            .map_err(|err| {
                observe::invalid_dto(&err, "auction");
                Error::EncodeAuctionError
            })?;

        let solutions = self
            .solutions
            .into_domain(&auction, &liquidity, weth, solver, &state.solver().config)
            .map_err(|err| {
                tracing::warn!(?err, "Failed to parse gas request into solutions");
                Error::EncodeSolutionError
            })?;
        Ok((auction, solutions))
    }
}

pub fn decimal_to_fee(fee: bigdecimal::BigDecimal) -> Result<liquidity::balancer::v2::Fee, Error> {
    let (value, _) = fee.into_bigint_and_exponent();
    let fee_u256 = U256Ext::from_big_int(&value).map_err(|_| Error::LiquidityError)?;
    Ok(liquidity::balancer::v2::Fee::from_raw(fee_u256))
}

fn decimal_to_weight(weight: bigdecimal::BigDecimal) -> liquidity::balancer::v2::weighted::Weight {
    let (value, _) = weight.into_bigint_and_exponent();
    let weight_u256 = U256Ext::from_big_int(&value).unwrap();
    liquidity::balancer::v2::weighted::Weight::from_raw(weight_u256)
}

fn decimal_to_scaling_factor(
    scaling_factor: bigdecimal::BigDecimal,
) -> Result<liquidity::balancer::v2::ScalingFactor, Error> {
    let (value, _) = scaling_factor.into_bigint_and_exponent();
    let scaling_factor_u256 = U256Ext::from_big_int(&value).unwrap();
    let scaling_factor: balancer::v2::ScalingFactor =
        liquidity::balancer::v2::ScalingFactor::from_raw(scaling_factor_u256)
            .map_err(|_| Error::LiquidityError)?;
    Ok(scaling_factor)
}

fn decimal_to_amp_param(
    amp_param: bigdecimal::BigDecimal,
) -> Result<liquidity::balancer::v2::stable::AmplificationParameter, Error> {
    let (value, _) = amp_param.into_bigint_and_exponent();
    let amp_param_u256 = U256Ext::from_big_int(&value).unwrap();
    let amp_param =
        liquidity::balancer::v2::stable::AmplificationParameter::new(amp_param_u256, 1000.into())
            .map_err(|_| Error::LiquidityError)?;
    Ok(amp_param)
}

fn decimal_to_v3_fee(fee: bigdecimal::BigDecimal) -> Result<uniswap::v3::Fee, Error> {
    let (value, _) = fee.with_scale(6).into_bigint_and_exponent();
    let fee_u256: eth::U256 = U256Ext::from_big_int(&value).map_err(|_| Error::LiquidityError)?;
    // tracing::info!("parsed fee: {}, exponent: {}", fee_u256, exp);
    Ok(uniswap::v3::Fee(Ratio::new(
        fee_u256.as_u32(),
        1_000_000u32,
    )))
}
